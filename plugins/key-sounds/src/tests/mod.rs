use super::*;
use std::collections::BTreeSet;

mod assets_and_groups;
mod configuration_and_activity;
mod views_and_preview;

/// The label of one numeric setting the settings panel publishes.
fn integer_label<'a>(output: &'a PluginOutput, key: &str) -> &'a str {
    match output.settings.nodes.iter().find_map(|node| match node {
        UiNode::Integer {
            key: node_key,
            label,
            ..
        } if node_key == key => Some(label.as_str()),
        _ => None,
    }) {
        Some(label) => label,
        None => panic!("the settings panel always publishes the {key} field"),
    }
}

/// The label of one dropdown the settings panel publishes.
fn select_label<'a>(output: &'a PluginOutput, key: &str) -> &'a str {
    match output.settings.nodes.iter().find_map(|node| match node {
        UiNode::Select {
            key: node_key,
            label,
            ..
        } if node_key == key => Some(label.as_str()),
        _ => None,
    }) {
        Some(label) => label,
        None => panic!("the settings panel always publishes the {key} dropdown"),
    }
}

/// The option one dropdown publishes for a value.
fn select_option<'a>(output: &'a PluginOutput, key: &str, value: &str) -> &'a SelectOption {
    match output.settings.nodes.iter().find_map(|node| match node {
        UiNode::Select {
            key: node_key,
            options,
            ..
        } if node_key == key => options.iter().find(|option| option.value == value),
        _ => None,
    }) {
        Some(option) => option,
        None => panic!("the settings panel always publishes the {key} dropdown"),
    }
}

/// The category heading one dropdown publishes for an option.
fn select_option_group<'a>(output: &'a PluginOutput, key: &str, value: &str) -> &'a str {
    match output.settings.nodes.iter().find_map(|node| match node {
        UiNode::Select {
            key: node_key,
            options,
            ..
        } if node_key == key => options
            .iter()
            .find(|option| option.value == value)
            .and_then(|option| option.group.as_deref()),
        _ => None,
    }) {
        Some(group) => group,
        None => panic!("the settings panel always groups the {key} option {value}"),
    }
}

/// The one failure message the settings panel publishes, if it published one.
fn error_text(output: &PluginOutput) -> &str {
    output
        .errors
        .first()
        .map(|error| error.message.as_str())
        .expect("the panel publishes the failure it is reporting")
}
