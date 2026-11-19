//! The environment as a layer under config.yaml. A key the config leaves unset and a variable
//! backs is read from that variable here, once, so nothing past this crate asks which of the two
//! set it.
//!
//! One rule for every such key: the config's own value first, then `LITELLM_<KEY>`, then the
//! default. The proxy reads some of these variables only when it starts, before the config's
//! `environment_variables` are exported, and reads a word that is not a flag as on. Neither is
//! kept: the variable is read from whatever `environment` is given, and its text is read as the
//! key's own text would be.
//!
//! `reasoning_auto_summary` is the one key the proxy reads the other way: it is on when its config
//! value or its variable is, so a variable that is on also fills a key the config sets to `false`.

use std::collections::BTreeMap;

use litellm_env::Lookup;
use litellm_secret_value::SecretValue;

use crate::{Config, Flag, LiteLlmSettings, Spelled};

/// Which config keys the environment set, each with the variable it was read from.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Origins(BTreeMap<&'static str, String>);

impl Origins {
    /// The variable that set the config key `key`, as `litellm_settings.drop_params` names one,
    /// when the environment set it.
    pub fn variable(&self, key: &str) -> Option<&str> {
        self.0.get(key).map(String::as_str)
    }
}

/// What a key holds when its variable set it: the variable's text, as the key's own text would
/// be read.
trait Written {
    fn written(text: String) -> Self;
}

impl<T> Written for Spelled<T> {
    fn written(text: String) -> Self {
        Spelled::Text(text)
    }
}

impl Written for String {
    fn written(text: String) -> Self {
        text
    }
}

impl Written for SecretValue {
    fn written(text: String) -> Self {
        Self::new(text)
    }
}

/// The config's `own` value of `key` when it set one, else the text of `variable` with the key
/// and variable it was read from. A blank variable is unset.
fn filled<T: Written>(
    own: Option<T>,
    environment: &dyn Lookup,
    key: &'static str,
    variable: String,
) -> (Option<T>, Option<(&'static str, String)>) {
    if own.is_some() {
        return (own, None);
    }
    match environment
        .get(&variable)
        .filter(|text| !text.trim().is_empty())
    {
        Some(text) => (Some(T::written(text)), Some((key, variable))),
        None => (None, None),
    }
}

/// `filled`, for a key that is on when its config value or its variable is: a variable that is
/// on also fills a key the config sets to `false`.
fn either(
    own: Option<Spelled<bool>>,
    environment: &dyn Lookup,
    key: &'static str,
    variable: String,
) -> (Option<Spelled<bool>>, Option<(&'static str, String)>) {
    let on = environment
        .get(&variable)
        .is_some_and(|text| Flag::read(&text).0);
    let kept = own.filter(|own| !(on && *own == Spelled::Value(false)));
    filled(kept, environment, key, variable)
}

/// `$settings` with each of the fields `$fill` reads from `LITELLM_<FIELD>`, and the key and
/// variable of each one that was.
macro_rules! layered {
    ($section:ident, $settings:expr, $environment:expr, $key:literal, $fill:ident, [$($field:ident),+ $(,)?]) => {{
        let settings = $settings;
        $(
            let $field = $fill(
                settings.$field,
                $environment,
                concat!($key, ".", stringify!($field)),
                concat!("LITELLM_", stringify!($field)).to_uppercase(),
            );
        )+
        let set = [$($field.1),+];
        ($section { $($field: $field.0,)+ ..settings }, set)
    }};
}

impl Config {
    /// This config with `environment` under it.
    pub fn with_environment(self, environment: &dyn Lookup) -> Self {
        let (settings, set) = layered!(
            LiteLlmSettings,
            self.litellm_settings,
            environment,
            "litellm_settings",
            filled,
            [
                drop_params,
                modify_params,
                enable_anthropic_prompt_caching,
                use_chat_completions_url_for_anthropic_messages,
                route_all_chat_openai_to_responses,
                gemini_live_defer_setup,
                anthropic_prompt_caching_ttl,
                model_cost_map_url,
                blog_posts_url,
                anthropic_beta_headers_url,
                autorouter_presets_url,
            ]
        );
        let (litellm_settings, set_either) = layered!(
            LiteLlmSettings,
            settings,
            environment,
            "litellm_settings",
            either,
            [reasoning_auto_summary]
        );
        Self {
            litellm_settings,
            origins: Origins(
                self.origins
                    .0
                    .into_iter()
                    .chain(set.into_iter().chain(set_either).flatten())
                    .collect(),
            ),
            ..self
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    const KEY: &str = "litellm_settings.drop_params";

    fn layered(yaml: &str, written: Option<&str>) -> Config {
        let lookup = |name: &str| {
            (name == "LITELLM_DROP_PARAMS")
                .then_some(written)
                .flatten()
                .map(str::to_owned)
        };
        Config::from_yaml(yaml).unwrap().with_environment(&lookup)
    }

    #[rstest]
    #[case::the_variable_when_the_config_leaves_the_key_unset(
        "model_list: []\n",
        Some("true"),
        Some(Spelled::Text("true".into())),
        Some("LITELLM_DROP_PARAMS")
    )]
    #[case::the_config_when_both_set_it(
        "litellm_settings:\n  drop_params: false\n",
        Some("true"),
        Some(Spelled::Value(false)),
        None
    )]
    #[case::neither("model_list: []\n", None, None, None)]
    #[case::a_blank_variable("model_list: []\n", Some("  "), None, None)]
    fn a_key_is_read_from_the_config_first_and_its_variable_second(
        #[case] yaml: &str,
        #[case] written: Option<&str>,
        #[case] value: Option<Spelled<bool>>,
        #[case] variable: Option<&str>,
    ) {
        let config = layered(yaml, written);

        assert_eq!(
            (
                config.litellm_settings.drop_params,
                config.origins.variable(KEY)
            ),
            (value, variable)
        );
    }

    #[rstest]
    fn a_variable_is_named_after_its_key() {
        let lookup = |name: &str| {
            (name == "LITELLM_USE_CHAT_COMPLETIONS_URL_FOR_ANTHROPIC_MESSAGES")
                .then(|| "true".to_owned())
        };

        let config = Config::default().with_environment(&lookup);

        assert_eq!(
            config
                .litellm_settings
                .use_chat_completions_url_for_anthropic_messages,
            Some(Spelled::Text("true".into()))
        );
    }

    #[rstest]
    fn a_text_key_holds_its_variables_text() {
        let lookup =
            |name: &str| (name == "LITELLM_ANTHROPIC_PROMPT_CACHING_TTL").then(|| "1h".to_owned());

        let config = Config::default().with_environment(&lookup);

        assert_eq!(
            (
                config
                    .litellm_settings
                    .anthropic_prompt_caching_ttl
                    .as_deref(),
                config
                    .origins
                    .variable("litellm_settings.anthropic_prompt_caching_ttl")
            ),
            (Some("1h"), Some("LITELLM_ANTHROPIC_PROMPT_CACHING_TTL"))
        );
    }

    #[rstest]
    #[case::off_in_the_config_and_on_in_the_variable(
        "false",
        "true",
        Some(Spelled::Text("true".into())),
        Some("LITELLM_REASONING_AUTO_SUMMARY")
    )]
    #[case::off_in_both("false", "false", Some(Spelled::Value(false)), None)]
    #[case::on_in_both("true", "true", Some(Spelled::Value(true)), None)]
    fn reasoning_auto_summary_is_on_when_its_key_or_its_variable_is(
        #[case] own: &str,
        #[case] written: &str,
        #[case] value: Option<Spelled<bool>>,
        #[case] variable: Option<&str>,
    ) {
        let lookup =
            |name: &str| (name == "LITELLM_REASONING_AUTO_SUMMARY").then(|| written.to_owned());
        let yaml = format!("litellm_settings:\n  reasoning_auto_summary: {own}\n");

        let config = Config::from_yaml(&yaml).unwrap().with_environment(&lookup);

        assert_eq!(
            (
                config.litellm_settings.reasoning_auto_summary,
                config
                    .origins
                    .variable("litellm_settings.reasoning_auto_summary")
            ),
            (value, variable)
        );
    }

    #[rstest]
    fn a_second_environment_keeps_what_the_first_one_set() {
        let first = |name: &str| (name == "LITELLM_DROP_PARAMS").then(|| "true".to_owned());
        let second = |name: &str| (name == "LITELLM_MODIFY_PARAMS").then(|| "true".to_owned());

        let config = Config::default()
            .with_environment(&first)
            .with_environment(&second);

        assert_eq!(
            (
                config.origins.variable(KEY),
                config.origins.variable("litellm_settings.modify_params")
            ),
            (Some("LITELLM_DROP_PARAMS"), Some("LITELLM_MODIFY_PARAMS"))
        );
    }

    #[rstest]
    #[case::the_config(
        "litellm_settings:\n  model_cost_map_url: https://user:credential-value@a.example/map.json\n",
        None
    )]
    #[case::its_variable(
        "model_list: []\n",
        Some("https://user:credential-value@a.example/map.json")
    )]
    fn a_url_key_is_hidden_whichever_of_the_two_set_it(
        #[case] yaml: &str,
        #[case] written: Option<&str>,
    ) {
        let lookup = |name: &str| {
            (name == "LITELLM_MODEL_COST_MAP_URL")
                .then_some(written)
                .flatten()
                .map(str::to_owned)
        };

        let config = Config::from_yaml(yaml).unwrap().with_environment(&lookup);
        let debug = format!("{:?}", config.litellm_settings);

        assert!(!debug.contains("credential-value"), "{debug}");
        assert!(debug.contains("[REDACTED]"), "{debug}");
    }
}
