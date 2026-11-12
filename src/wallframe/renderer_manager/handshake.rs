use super::*;

/// Init settings keys that tell a renderer the pixel size of the output it is shown on, so one that
/// draws at the output's aspect (a perspective scene) needs no aspect configured by hand.
pub const OUTPUT_WIDTH_KEY: &str = "output_width";
pub const OUTPUT_HEIGHT_KEY: &str = "output_height";

/// Puts the output size into spawn settings; a zero size (unknown) leaves them alone.
pub fn set_output_extent(settings: &mut HashMap<String, String>, width: u32, height: u32) {
    if width == 0 || height == 0 {
        return;
    }
    settings.insert(OUTPUT_WIDTH_KEY.to_string(), width.to_string());
    settings.insert(OUTPUT_HEIGHT_KEY.to_string(), height.to_string());
}

pub(crate) fn build_init_msg(req: &SpawnRequest, def: &RendererDef) -> ControlMsg {
    let mut settings_kv: HashMap<String, String> = req.settings.clone();

    if def.settings.contains_key("test_pattern") && req.test_pattern {
        settings_kv.insert("test_pattern".to_string(), "1".to_string());
    }

    let mut settings: Vec<(String, String)> = settings_kv.into_iter().collect();
    settings.sort_by(|a, b| a.0.cmp(&b.0));

    ControlMsg::Init {
        config: RendererInit {
            protocol_version: crate::wallframe::ipc::proto::PROTOCOL_VERSION,
            spawn_version: SPAWN_VERSION,
            settings,
            user_properties: crate::catalog::properties::merge_renderer_user_properties(
                &req.default_user_properties,
                &req.user_property_overrides,
            ),
        },
    }
}

pub(super) fn validate_renderer_spawn_version(def: &RendererDef) -> Result<()> {
    let declared = def.spawn_version.unwrap_or(SPAWN_VERSION);
    if declared == SPAWN_VERSION {
        return Ok(());
    }
    Err(Error::RendererSpawnFailed(format!(
        "renderer spawn version mismatch: plugin '{}' renderer '{}' declares {declared}, daemon requires {SPAWN_VERSION}; update the plugin",
        def.plugin_id, def.name
    )))
}

pub(crate) fn run_init_handshake(sock: &StdUnixStream, init: &ControlMsg) -> Result<DrmNode> {
    send_control(sock, init, &[])
        .map_err(|error| Error::RendererSpawnFailed(format!("send Init: {error}")))?;
    let (event, fds) = recv_event(sock)
        .map_err(|error| Error::RendererSpawnFailed(format!("recv Ready: {error}")))?;
    match event {
        EventMsg::Ready { drm_node } => {
            if !fds.is_empty() {
                log::warn!("Ready unexpectedly carried {} fds; dropping", fds.len());
            }
            Ok(DrmNode {
                major: drm_node.major,
                minor: drm_node.minor,
            })
        }
        EventMsg::InitNack { rejection } => Err(Error::RendererSpawnFailed(format!(
            "renderer rejected Init: {} (protocol {} supported {}; spawn {} supported {})",
            rejection.reason,
            rejection.received_protocol_version,
            rejection.supported_protocol_version,
            rejection.received_spawn_version,
            rejection.supported_spawn_version,
        ))),
        other => Err(Error::RendererSpawnFailed(format!(
            "host emitted {other:?} before Ready; aborting spawn"
        ))),
    }
}

#[cfg(test)]
mod output_extent_tests {
    use super::*;

    #[test]
    fn output_extent_lands_in_the_settings() {
        let mut settings = HashMap::new();
        set_output_extent(&mut settings, 5120, 1440);
        assert_eq!(settings[OUTPUT_WIDTH_KEY], "5120");
        assert_eq!(settings[OUTPUT_HEIGHT_KEY], "1440");
    }

    #[test]
    fn an_unknown_output_size_adds_nothing_and_keeps_what_was_there() {
        let mut settings = HashMap::from([(OUTPUT_WIDTH_KEY.to_string(), "1920".to_string())]);
        set_output_extent(&mut settings, 0, 1440);
        set_output_extent(&mut settings, 5120, 0);
        assert_eq!(settings.len(), 1);
        assert_eq!(settings[OUTPUT_WIDTH_KEY], "1920");
    }
}
