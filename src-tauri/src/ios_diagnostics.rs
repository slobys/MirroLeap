//! A bounded timeline of iOS connection states, without device identifiers or input contents.

use serde::Serialize;
use std::collections::VecDeque;
use std::sync::{LazyLock, Mutex};

const HISTORY_LIMIT: usize = 256;
static TIMELINE: LazyLock<Mutex<Timeline>> = LazyLock::new(|| Mutex::new(Timeline::default()));

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Channel {
    Usb,
    UsbVideo,
    AirplayVideo,
    Bluetooth,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub usb: &'static str,
    pub usb_video: &'static str,
    pub airplay_video: &'static str,
    pub bluetooth: &'static str,
    pub bluetooth_transport: Option<&'static str>,
}

impl Default for Status {
    fn default() -> Self {
        Self {
            usb: "starting",
            usb_video: "idle",
            airplay_video: "idle",
            bluetooth: "off",
            bluetooth_transport: None,
        }
    }
}

#[derive(Clone, Copy, Serialize)]
pub(crate) struct CaptureFailure {
    pub kind: i32,
    pub stage: i32,
    pub code: i32,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Event {
    time_utc: String,
    channel: Channel,
    state: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    transport: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    failure: Option<CaptureFailure>,
}

#[derive(Default)]
struct Timeline {
    current: Status,
    events: VecDeque<Event>,
}

impl Timeline {
    fn push(
        &mut self,
        channel: Channel,
        state: &'static str,
        transport: Option<&'static str>,
        failure: Option<CaptureFailure>,
    ) {
        if self.events.len() == HISTORY_LIMIT {
            self.events.pop_front();
        }
        self.events.push_back(Event {
            time_utc: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            channel,
            state,
            transport,
            failure,
        });
    }

    fn observe(&mut self, channel: Channel, state: &'static str, transport: Option<&'static str>) {
        let previous = self.current;
        match channel {
            Channel::Usb => self.current.usb = state,
            Channel::UsbVideo => self.current.usb_video = state,
            Channel::AirplayVideo => self.current.airplay_video = state,
            Channel::Bluetooth => {
                self.current.bluetooth = state;
                self.current.bluetooth_transport = transport;
            }
        }
        if previous != self.current {
            self.push(channel, state, transport, None);
        }
    }
}

pub(crate) fn observe(channel: Channel, state: &'static str, transport: Option<&'static str>) {
    if let Ok(mut timeline) = TIMELINE.lock() {
        timeline.observe(channel, state, transport);
    }
}

pub(crate) fn usb_failure(failure: CaptureFailure) {
    if let Ok(mut timeline) = TIMELINE.lock() {
        timeline.push(Channel::UsbVideo, "capture_failure", None, Some(failure));
    }
}

pub(crate) fn status() -> Status {
    TIMELINE
        .lock()
        .map(|timeline| timeline.current)
        .unwrap_or_default()
}

pub(crate) fn report(version: &str) -> Result<String, String> {
    let timeline = TIMELINE.lock().map_err(|err| err.to_string())?;
    serde_json::to_string_pretty(&serde_json::json!({
        "schemaVersion": 1,
        "appVersion": version,
        "exportedAtUtc": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        "status": timeline.current,
        "events": timeline.events,
    }))
    .map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_video_pause_and_resume_preserve_usb_and_control_status() {
        let mut timeline = Timeline::default();
        timeline.observe(Channel::Usb, "streaming", None);
        timeline.observe(Channel::Bluetooth, "ready", Some("usb"));
        timeline.observe(Channel::UsbVideo, "receiving", None);
        timeline.observe(Channel::UsbVideo, "paused", None);
        timeline.observe(Channel::UsbVideo, "paused", None);
        assert_eq!(timeline.current.usb, "streaming");
        assert_eq!(timeline.current.bluetooth, "ready");
        timeline.observe(Channel::UsbVideo, "recovering", None);
        timeline.observe(Channel::UsbVideo, "receiving", None);
        assert_eq!(timeline.events.len(), 6);
        assert_eq!(timeline.current.usb_video, "receiving");
    }

    #[test]
    fn the_report_keeps_only_the_latest_events_and_numeric_failure_codes() {
        let mut timeline = Timeline::default();
        timeline.observe(Channel::AirplayVideo, "waiting", None);
        for _ in 0..HISTORY_LIMIT {
            timeline.push(
                Channel::UsbVideo,
                "capture_failure",
                None,
                Some(CaptureFailure {
                    kind: 3,
                    stage: 5,
                    code: -5,
                }),
            );
        }
        assert_eq!(timeline.events.len(), HISTORY_LIMIT);
        let events = serde_json::to_value(&timeline.events).unwrap();
        assert_eq!(events[0]["channel"], "usb_video");
        assert_eq!(events[0]["failure"]["code"], -5);
        assert!(events[0].get("transport").is_none());
    }
}
