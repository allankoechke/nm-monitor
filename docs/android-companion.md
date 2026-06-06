# Android Companion App (Phase 3)

The `nm-notify` crate includes an `FcmChannel` stub. When implementing the companion app:

## Agent API integration

- Poll `GET /health` for agent name, SSID, and link state
- Subscribe to `GET /events/stream` (SSE) for live device/network events
- Use `GET /speedtests` for chart data

## FCM push flow

1. Companion app registers FCM token with agent via future `POST /fcm/register`
2. Agent stores token in SQLite
3. `FcmChannel` sends structured JSON matching `NotificationPayload`:
   - `agent_name`, `network_name`, `kind`, `title`, `body`, `timestamp`

## Notification filtering

Clients should filter on `agent_name` when multiple agents share one Firebase project.

Until FCM is implemented, use the [ntfy Android app](https://ntfy.sh) with your configured topic.
