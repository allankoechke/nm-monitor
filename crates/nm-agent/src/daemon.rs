use chrono::Utc;
use nm_api::{build_router, ApiState};
use nm_classify::{ClassificationInput, DeviceClassifier};
use nm_core::config::{
    default_agent_name_from_hostname, default_config_path, expand_path, load_config, save_config,
    AppConfig,
};
use nm_core::device::{Device, DeviceSnapshot};
use nm_core::event::EventKind;
use nm_core::notify::{
    device_joined_body, device_left_body, network_down_body, network_down_title,
    network_restored_body, network_restored_title, NotificationPayload,
};
use nm_discovery::{
    detect_network, DiscoveryBackend, LinkMonitor, LinkState, MdnsRegistry, NetworkContext,
    PassiveCapture, PassiveObservation, PlatformBackend,
};
use nm_store::RegistryEvent;
use nm_notify::NotificationDispatcher;
use nm_speedtest::{SpeedTestContext, SpeedTestScheduler};
use nm_store::{DeviceRegistry, Store};
use std::path::Path;
use std::sync::Arc;
use tokio::signal;
use tokio::sync::{broadcast, mpsc};
use tracing::{error, info};

pub async fn run_daemon(config_path: &Path, scan_once: bool) -> Result<(), Box<dyn std::error::Error>> {
    let mut config = if config_path.exists() {
        load_config(config_path)?
    } else {
        let expected = default_config_path();
        return Err(format!(
            "config not found at {}. Run `nm-agent setup` first (writes to {}).",
            config_path.display(),
            expected.display()
        )
        .into());
    };

    let agent_name = resolve_agent_name(&mut config, config_path)?;
    let db_path = expand_path(&config.storage.database_path);
    let store = Arc::new(Store::open(std::path::Path::new(&db_path))?);
    store.set_agent_name(&agent_name)?;

    let dispatcher = Arc::new(NotificationDispatcher::from_config(&config.notifications));
    let network_context = NetworkContext::new();
    let link_monitor = Arc::new(LinkMonitor::new(
        &config.network.interface,
        &config.network.gateway,
        config.network.link_check_interval_secs,
    ));

    let (event_tx, _) = broadcast::channel::<String>(256);
    let mut registry = DeviceRegistry::new(store.clone(), agent_name.clone());
    registry.set_agent_name(agent_name.clone());

    let network = detect_network(&config.network.interface, &config.network.gateway)
        .map_err(|e| format!("network detection failed: {e}"))?;
    let interface = network.interface.clone();
    network_context.set_interface(&interface);
    network_context.clone().spawn_refresh_task(interface.clone(), 60);

    let mdns_registry = MdnsRegistry::new();
    nm_discovery::spawn_mdns_browse(mdns_registry.clone());

    let speedtest_scheduler = Arc::new(SpeedTestScheduler::new(
        config.speedtest.clone(),
        SpeedTestContext {
            agent_name: agent_name.clone(),
            network_name: network_context.network_name(),
            interface: interface.clone(),
            network_up: true,
        },
    ));

    let api_state = ApiState {
        store: store.clone(),
        network_context: network_context.clone(),
        link_monitor: link_monitor.clone(),
        agent_name: Arc::new(parking_lot::RwLock::new(agent_name.clone())),
        speedtest: Some(speedtest_scheduler.clone()),
        event_tx: event_tx.clone(),
    };

    if config.api.enabled {
        let router = build_router(api_state);
        let bind_addr = config.api.bind_addr.clone();
        tokio::spawn(async move {
            let bind: std::net::SocketAddr = bind_addr
                .parse()
                .expect("invalid api.bind_addr");
            let listener = tokio::net::TcpListener::bind(bind).await.unwrap();
            info!(%bind, "API server listening");
            axum::serve(listener, router).await.unwrap();
        });
    }

    link_monitor.clone().spawn();

    let (passive_tx, mut passive_rx) = mpsc::channel::<PassiveObservation>(128);
    if config.network.passive_capture {
        PassiveCapture::new(&interface).spawn(passive_tx);
    }

    let store_speed = store.clone();
    let event_tx_speed = event_tx.clone();
    let agent_name_speed = agent_name.clone();
    speedtest_scheduler.clone().spawn(move |result| {
        if let Err(e) = store_speed.insert_speed_test(&result) {
            error!(error = %e, "failed to store speed test");
        }
        let _ = event_tx_speed.send(serde_json::json!({
            "kind": "speed_test_completed",
            "agent_name": agent_name_speed,
            "download_mbps": result.download_mbps,
            "upload_mbps": result.upload_mbps,
        }).to_string());
    });

    let backend = PlatformBackend::new(&config.network.interface, &config.network.gateway);
    let mut link_last = LinkState::Unknown;
    let mut scan_interval =
        tokio::time::interval(std::time::Duration::from_secs(config.network.scan_interval_secs.max(10)));

    info!(agent = %agent_name, interface = %interface, "nm-agent started");

    if scan_once {
        info!(interface = %interface, agent = %agent_name, "scan-once: starting network scan");
        let device_count = run_scan_cycle(
            &backend,
            &mut registry,
            &dispatcher,
            &network_context,
            &mdns_registry,
            &store,
            &event_tx,
            &link_monitor,
            &speedtest_scheduler,
            &interface,
            &mut link_last,
        )
        .await?;
        info!(
            interface = %interface,
            agent = %agent_name,
            device_count,
            "scan-once: scan completed"
        );
        return Ok(());
    }

    loop {
        tokio::select! {
            _ = scan_interval.tick() => {
                if let Err(e) = run_scan_cycle(
                    &backend,
                    &mut registry,
                    &dispatcher,
                    &network_context,
                    &mdns_registry,
                    &store,
                    &event_tx,
                    &link_monitor,
                    &speedtest_scheduler,
                    &interface,
                    &mut link_last,
                )
                .await
                {
                    error!(error = %e, "scan cycle failed");
                }
                let _ = registry.mark_stale_offline(
                    config.network.presence_timeout_secs,
                    network_context.network_name().as_deref(),
                );
            }
            Some(obs) = passive_rx.recv() => {
                let network_name = network_context.network_name();
                let snapshots = vec![obs.snapshot];
                if let Ok(events) = registry.process_sweep(&snapshots, network_name.as_deref()) {
                    for evt in events {
                        dispatch_registry_event(&dispatcher, &network_context, &registry, evt).await;
                    }
                }
            }
            _ = signal::ctrl_c() => {
                info!("shutting down");
                break;
            }
        }
    }
    Ok(())
}

async fn run_scan_cycle(
    backend: &PlatformBackend,
    registry: &mut DeviceRegistry,
    dispatcher: &NotificationDispatcher,
    network_context: &NetworkContext,
    mdns_registry: &MdnsRegistry,
    store: &Arc<Store>,
    event_tx: &broadcast::Sender<String>,
    link_monitor: &Arc<LinkMonitor>,
    speedtest_scheduler: &Arc<SpeedTestScheduler>,
    interface: &str,
    link_last: &mut LinkState,
) -> Result<usize, Box<dyn std::error::Error>> {
    let link = link_monitor.status();
    let network_name = network_context.network_name();

    speedtest_scheduler.update_context(SpeedTestContext {
        agent_name: registry.agent_name().to_string(),
        network_name: network_name.clone(),
        interface: interface.to_string(),
        network_up: link.state == LinkState::Up,
    });

    if link.state != *link_last && *link_last != LinkState::Unknown {
        handle_link_change(
            registry,
            dispatcher,
            network_name.as_deref(),
            link.state,
            link.gateway,
        )
        .await;
    }
    *link_last = link.state;

    if link.state == LinkState::Down {
        return Ok(0);
    }

    let snapshot = backend.discover().await?;
    network_context.set_interface(&snapshot.network.interface);

    let mut devices = snapshot.devices;
    for dev in &mut devices {
        if let Some(host) = &dev.hostname {
            let services = mdns_registry.services_for_host(host);
            if !services.is_empty() {
                if let Some(mut stored) = store.get_device(&dev.mac)? {
                    stored.mdns_services = services.clone();
                    let classification = DeviceClassifier::classify(&ClassificationInput {
                        mac: dev.mac,
                        vendor: dev.vendor.clone(),
                        hostname: dev.hostname.clone(),
                        open_ports: stored.open_ports.clone(),
                        mdns_services: services,
                        dhcp_hostname: None,
                        is_gateway: snapshot.network.gateway == dev.ip,
                    });
                    stored.kind = classification.kind;
                    stored.os_hint = classification.os_hint;
                    stored.confidence = classification.confidence;
                    stored.inference_source = Some(classification.inference_source);
                    store.upsert_device(&stored)?;
                }
            }
        }
    }

    let events = registry.process_sweep(&devices, network_name.as_deref())?;
    for evt in events {
        dispatch_registry_event(dispatcher, network_context, registry, evt).await;
    }

    log_discovered_devices(&devices, store, snapshot.network.gateway, mdns_registry)?;

    let device_count = devices.len();
    let _ = event_tx.send(serde_json::json!({
        "kind": "scan_completed",
        "device_count": device_count,
        "timestamp": Utc::now(),
    }).to_string());

    Ok(device_count)
}

fn log_discovered_devices(
    snapshots: &[DeviceSnapshot],
    store: &Store,
    gateway: Option<std::net::IpAddr>,
    mdns_registry: &MdnsRegistry,
) -> Result<(), Box<dyn std::error::Error>> {
    for snap in snapshots {
        let stored = store.get_device(&snap.mac)?;
        let mdns_services = snap
            .hostname
            .as_ref()
            .map(|host| mdns_registry.services_for_host(host))
            .unwrap_or_default();

        let (kind, os_hint) = if let Some(ref device) = stored {
            (device.kind, device.os_hint)
        } else {
            let classification = DeviceClassifier::classify(&ClassificationInput {
                mac: snap.mac,
                vendor: snap.vendor.clone(),
                hostname: snap.hostname.clone(),
                open_ports: Vec::new(),
                mdns_services,
                dhcp_hostname: None,
                is_gateway: gateway == snap.ip,
            });
            (classification.kind, classification.os_hint)
        };

        let identity_name = stored
            .as_ref()
            .and_then(|d| d.identity_id)
            .and_then(|id| store.get_identity(&id).ok().flatten())
            .map(|i| i.display_name);

        let name = stored
            .as_ref()
            .map(|d| d.display_name(identity_name.as_deref()))
            .unwrap_or_else(|| snapshot_display_name(snap));

        let device_type = Device {
            mac: snap.mac,
            current_ip: snap.ip,
            hostname: snap.hostname.clone(),
            vendor: snap.vendor.clone(),
            kind,
            os_hint,
            identity_id: None,
            user_label: None,
            first_seen: Utc::now(),
            last_seen: Utc::now(),
            online: true,
            open_ports: Vec::new(),
            mdns_services: Vec::new(),
            confidence: 0.0,
            inference_source: None,
            do_not_scan: false,
        }
        .kind_label();

        let ip = snap
            .ip
            .or_else(|| stored.as_ref().and_then(|d| d.current_ip))
            .map(|addr| addr.to_string())
            .unwrap_or_else(|| "unknown".into());

        info!(
            name = %name,
            device_type = %device_type,
            ip = %ip,
            mac = %snap.mac,
            "discovered device"
        );
    }
    Ok(())
}

fn snapshot_display_name(snap: &DeviceSnapshot) -> String {
    if let Some(host) = &snap.hostname {
        return host.clone();
    }
    if let Some(vendor) = &snap.vendor {
        return format!("{vendor} device");
    }
    snap.mac.to_string()
}

async fn handle_link_change(
    registry: &DeviceRegistry,
    dispatcher: &NotificationDispatcher,
    network_name: Option<&str>,
    state: LinkState,
    gateway: Option<std::net::IpAddr>,
) {
    let (kind, title, body) = match state {
        LinkState::Down => (
            EventKind::NetworkDown,
            network_down_title(network_name),
            network_down_body(gateway, network_name),
        ),
        LinkState::Up => (
            EventKind::NetworkRestored,
            network_restored_title(network_name),
            network_restored_body(network_name),
        ),
        LinkState::Unknown => return,
    };

    let payload = NotificationPayload {
        agent_name: registry.agent_name().to_string(),
        network_name: network_name.map(str::to_string),
        kind,
        title: title.clone(),
        body: body.clone(),
        timestamp: Utc::now(),
        device_name: None,
        device_ip: None,
        gateway,
    };
    dispatcher.dispatch(&payload).await;
    let _ = registry.record_event(kind, network_name, None, body, None);
}

async fn dispatch_registry_event(
    dispatcher: &NotificationDispatcher,
    network_context: &NetworkContext,
    registry: &DeviceRegistry,
    evt: RegistryEvent,
) {
    let network_name = network_context.network_name();
    match evt {
        RegistryEvent::DeviceJoined { device, identity_name } => {
            let name = device.display_name(identity_name.as_deref());
            let body = device_joined_body(
                &name,
                &device.kind_label(),
                device.current_ip,
                network_name.as_deref(),
            );
            let payload = NotificationPayload {
                agent_name: registry.agent_name().to_string(),
                network_name: network_name.clone(),
                kind: EventKind::DeviceJoined,
                title: format!("{name} joined"),
                body,
                timestamp: Utc::now(),
                device_name: Some(name),
                device_ip: device.current_ip,
                gateway: None,
            };
            dispatcher.dispatch(&payload).await;
        }
        RegistryEvent::DeviceReturned { device, identity_name } => {
            let name = device.display_name(identity_name.as_deref());
            let body = device_joined_body(
                &name,
                &device.kind_label(),
                device.current_ip,
                network_name.as_deref(),
            );
            let payload = NotificationPayload {
                agent_name: registry.agent_name().to_string(),
                network_name: network_name.clone(),
                kind: EventKind::DeviceReturned,
                title: format!("{name} returned"),
                body,
                timestamp: Utc::now(),
                device_name: Some(name),
                device_ip: device.current_ip,
                gateway: None,
            };
            dispatcher.dispatch(&payload).await;
        }
        RegistryEvent::DeviceLeft { device, identity_name } => {
            let name = device.display_name(identity_name.as_deref());
            let body = device_left_body(&name, &device.kind_label(), network_name.as_deref());
            let payload = NotificationPayload {
                agent_name: registry.agent_name().to_string(),
                network_name: network_name.clone(),
                kind: EventKind::DeviceLeft,
                title: format!("{name} left"),
                body,
                timestamp: Utc::now(),
                device_name: Some(name),
                device_ip: device.current_ip,
                gateway: None,
            };
            dispatcher.dispatch(&payload).await;
        }
        RegistryEvent::IpChanged { .. } => {}
    }
}

fn resolve_agent_name(
    config: &mut AppConfig,
    config_path: &Path,
) -> Result<String, Box<dyn std::error::Error>> {
    if let Some(name) = &config.agent.name {
        if !name.trim().is_empty() {
            return Ok(name.clone());
        }
    }
    let name = default_agent_name_from_hostname();
    config.agent.name = Some(name.clone());
    if config_path.parent().map(|p| p.exists()).unwrap_or(true) {
        let _ = save_config(config_path, config);
    }
    Ok(name)
}
