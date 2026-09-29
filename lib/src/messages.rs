//! User-facing texts for FDP results. Keep in sync with ui/src/shared/messages.ts,
//! which the frontend uses for the same statuses.

fn status_text(status: &str) -> Option<&'static str> {
    Some(match status {
        "ok" => "Done.",
        "auth_failed" => "Wrong user name or password.",
        "bad_request" => {
            "The camera could not read the request. The app and the camera firmware probably implement \
             different versions of the discovery protocol. Update the app or the camera firmware."
        }
        "invalid_config" => "The camera rejected the network settings.",
        "disabled" => "Changing the IP address via the discovery tool is disabled in the camera's web interface.",
        "locked" => "Too many failed attempts. Please wait 5 minutes and try again.",
        "error" => "The camera reported an internal error and did not change its settings.",
        "timeout" => "The camera did not respond. Check the connection and try again.",
        "untrusted" => "The camera key was not confirmed. Nothing was sent.",
        "key_changed" => {
            "The camera key changed while the dialog was open. Nothing was sent. Please check the key and try again."
        }
        "unsupported_version" => "The camera firmware and this app speak different versions of the discovery protocol.",
        _ => return None,
    })
}

/// Protocol versions of camera and app, for the "unsupported version" advice.
pub struct VersionInfo<'a> {
    pub camera: &'a [u32],
    pub app: &'a [u32],
}

fn version_advice(v: &VersionInfo) -> String {
    let list = |xs: &[u32]| xs.iter().map(|x| format!("v{x}")).collect::<Vec<_>>().join(", ");
    let max = |xs: &[u32]| xs.iter().copied().max().unwrap_or(0);
    let update = if max(v.camera) < max(v.app) { "Update the camera firmware." } else { "Update this app." };
    format!("Camera: {}, app: {}. {update}", list(v.camera), list(v.app))
}

/// Explanation for a status, plus the camera's own message if it adds anything.
pub fn status_message(status: &str, detail: Option<&str>, versions: Option<VersionInfo>) -> String {
    let mut text = match status_text(status) {
        Some(t) => t.to_string(),
        None => format!("The camera answered with an unknown status \"{status}\"."),
    };
    if let Some(v) = versions {
        text = format!("{text} {}", version_advice(&v));
    }
    match detail {
        Some(d) if !d.is_empty() && d != text => format!("{text}\nCamera message: {d}"),
        _ => text,
    }
}

pub fn version_message(camera_versions: &[u32]) -> String {
    status_message(
        "unsupported_version",
        None,
        Some(VersionInfo { camera: camera_versions, app: fdp::CLIENT_VERSIONS }),
    )
}
