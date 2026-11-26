use super::*;

impl KeySounds {
    /// One select node for a device's own sound.
    pub(super) fn device_select(&self, category: PackCategory, label: &str) -> UiNode {
        let options = category
            .packs()
            .map(|pack| SelectOption {
                value: pack.id.into(),
                // A set with no release recording still has press sounds, but
                // choosing it makes the release edge silent.
                label: if !pack.supports_key_up {
                    format!("{} · {}", pack.label, self.copy().no_release)
                } else {
                    pack.label.into()
                },
                group: Some(pack.brand.into()),
            })
            .collect();
        let (key, value) = match category {
            PackCategory::Keyboard => ("keyboard_pack", self.selection.keyboard.id),
            PackCategory::Mouse => ("mouse_pack", self.selection.mouse.id),
        };
        UiNode::Select {
            key: key.into(),
            label: label.into(),
            value: value.into(),
            options,
            preview: true,
        }
    }

    pub(super) fn output(&self) -> PluginOutput {
        let copy = self.copy();
        let nodes = vec![
            UiNode::Integer {
                key: "volume".into(),
                label: copy.volume.into(),
                value: self.volume,
                min: MIN_VOLUME,
                max: MAX_VOLUME,
            },
            UiNode::Toggle {
                key: "mouse_enabled".into(),
                label: copy.mouse_enabled.into(),
                value: self.mouse_enabled,
            },
            self.device_select(PackCategory::Keyboard, copy.keyboard_pack),
            self.device_select(PackCategory::Mouse, copy.mouse_pack),
        ];
        let config = Configuration::from([
            ("volume".into(), ConfigValue::Integer(self.volume)),
            (
                "mouse_enabled".into(),
                ConfigValue::Boolean(self.mouse_enabled),
            ),
            (
                "keyboard_pack".into(),
                ConfigValue::Text(self.selection.keyboard.id.into()),
            ),
            (
                "mouse_pack".into(),
                ConfigValue::Text(self.selection.mouse.id.into()),
            ),
        ]);
        PluginOutput {
            errors: self
                .error
                .map(|error| bongocat_plugin_sdk::PluginError {
                    id: match error {
                        AudioError::Decode => "audio_decode",
                        AudioError::Output => "audio_output",
                    }
                    .into(),
                    message: error.copy(copy).into(),
                })
                .into_iter()
                .collect(),
            settings: PluginView {
                nodes,
                ..Default::default()
            },
            config,
            ..Default::default()
        }
    }
}
