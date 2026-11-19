## Scope

- This crate is the typed data model of a config and reads no file. Reading files and `include` live in `config-load`, which hands the merged document to `Config::from_value`

## Redis settings

- This crate only deserializes YAML. `RedisSettings` is the raw `coordination_redis` or `cache_params` map and `CacheSettings` tells a `type: redis` cache apart from the rest. Reading those maps the way `litellm/_redis.py` does and turning the merged kwargs into a `Resolution` both belong to `redis-resolve`

## Route settings

- `RouteList` is the typed form of a route list in config and key metadata, and both hosts read through it. A value that is not a list of strings is `RouteList::Malformed`, which grants nothing. A route template inside such a list is starlette syntax, so it is parsed by `litellm-starlette`, never here

## Environment

- `Environment` is the settings only the environment holds, the variables the proxy reads that no config.yaml key sets, one field per variable, named as the variable in lower case. The host deserializes it once and hands it to whatever builds settings from it
- `Environment::read` asks a `Lookup` for each variable by its field's name. Use it for an environment that cannot be listed, as a config's `environment_variables` over a secret manager is
- A setting config.yaml can also set is never a field here. It is a field of its config section, and `Config::with_environment` in `layer.rs` fills it when the config leaves it unset, so nothing past this crate asks which of the two set it. The rule is the same for every such key but one: the config's value, then `LITELLM_<KEY>`, then the default. `reasoning_auto_summary` is on when its key or its variable is, as `is_reasoning_auto_summary_enabled` reads it, so a variable that is on also fills a key the config sets to `false`. `config.origins` keeps which keys the environment set, for a log or an error that has to name the variable
- The layer does not copy two things the proxy does. It reads the variable from the `Lookup` it is given, which `config-load` makes the config's `environment_variables` over the process environment, where the proxy reads some of these only at import. And it keeps the variable's text for the key's own reader, where the proxy reads a word that is not a flag as on
- Values stay as written: a comma-separated list is a `String`, an optional number is an `Option`. Splitting a list, applying a default and validating belong to the consumer
- `Flag` is the one parsed shape, because every consumer would otherwise repeat Python's `strip().lower() == "true"`. Python string semantics it needs come from `python-compat`
