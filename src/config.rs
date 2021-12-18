use crate::input::{
    ButtonMode, FieldType, InputConfig, Key, OutputConfig, StateInput, StateOutput,
};
use anyhow::{Context, Result};

#[derive(Debug)]
pub struct Config {
    pub midi_in_name: Option<String>,
    pub midi_out_name: Option<String>,
    pub rack_name: String,
    keys: toml::map::Map<String, toml::value::Value>,
}
pub fn load_config(path: &str) -> Result<Config> {
    use std::io::Read;
    use toml::Value;

    let mut file = std::fs::File::open(path)?;
    let mut content = String::new();
    file.read_to_string(&mut content)?;
    let parsed = content.parse::<Value>()?;
    let midi_in_name = parsed.get("default").and_then(|d| d.get("input"));
    let midi_in_name = match midi_in_name {
        Some(x) => Some(
            x.as_str()
                .context("Type error at default.input")?
                .to_owned(),
        ),
        None => None,
    };
    let midi_out_name = parsed.get("default").and_then(|d| d.get("output"));
    let midi_out_name = match midi_out_name {
        Some(x) => Some(
            x.as_str()
                .context("Type error at default.output")?
                .to_owned(),
        ),
        None => None,
    };
    let rack_name = parsed
        .get("rack")
        .and_then(|d| d.get("name"))
        .context("rack.name is not defined")?
        .as_str()
        .context("Type error at rack.name")?
        .to_owned();
    let keys = parsed
        .get("keys")
        .and_then(|d| d.as_table())
        .cloned()
        .unwrap_or_else(toml::map::Map::new);
    Ok(Config {
        midi_in_name,
        midi_out_name,
        rack_name,
        keys,
    })
}

pub fn setup_state_io<S>(
    config: &Config,
    state_in: &mut StateInput<S>,
    state_out: &mut StateOutput<S>,
) -> Result<()> {
    for (name, value) in config.keys.iter() {
        match state_in.field_type(name) {
            None => {
                anyhow::bail!("Field not defined: {}", name);
            }
            Some(FieldType::F32) => {
                let key = value
                    .as_integer()
                    .ok_or_else(|| anyhow::anyhow!("Type error at keys.{}", name))?;
                let key = Key::ControlChange(key as u8);
                state_in.define_input(
                    key,
                    InputConfig::F32 {
                        name: name.to_owned(),
                    },
                )
            }
            Some(FieldType::Bool) => {
                let value = value
                    .as_table()
                    .ok_or_else(|| anyhow::anyhow!("Type error at keys.{}", name))?;
                let key = match value.get("key") {
                    None => None,
                    Some(x) => Some(
                        x.as_integer()
                            .ok_or_else(|| anyhow::anyhow!("Type error at keys.{}.key", name))?,
                    ),
                };
                let key = key.map(|x| Key::ControlChange(x as u8));
                if let Some(key) = key {
                    let mode = match value.get("mode") {
                        Some(toml::value::Value::String(s)) => match s.as_ref() {
                            "toggle" => ButtonMode::Toggle,
                            "momentary" => ButtonMode::Momentary,
                            _ => return Err(anyhow::anyhow!("Invalid mode at keys.{}.mode", name)),
                        },
                        Some(_) => return Err(anyhow::anyhow!("Type error at keys.{}.mode", name)),
                        None => return Err(anyhow::anyhow!("keys.{}.mode required", name)),
                    };
                    state_in.define_input(
                        key,
                        InputConfig::Bool {
                            name: name.to_owned(),
                            mode,
                        },
                    );
                }
                let out = match value.get("out") {
                    Some(toml::value::Value::Integer(n)) => Some(Key::ControlChange(*n as u8)),
                    Some(_) => return Err(anyhow::anyhow!("Type error at keys.{}.out", name)),
                    None => None,
                };
                if let Some(out) = out {
                    state_out.define_output(OutputConfig::Bool {
                        name: name.to_owned(),
                        out,
                    });
                }
            }
            Some(FieldType::Enum) => {
                let value = value
                    .as_array()
                    .ok_or_else(|| anyhow::anyhow!("Type error at keys.{}", name))?;
                for (i, v) in value.iter().enumerate() {
                    let v = v
                        .as_table()
                        .ok_or_else(|| anyhow::anyhow!("Type error at keys.{}[{}]", name, i))?;
                    let key = v.get("key");
                    let key = match key.map(|x| x.as_integer()) {
                        Some(Some(v)) => Some(Key::ControlChange(v as u8)),
                        Some(None) => {
                            return Err(anyhow::anyhow!("Type error at keys.{}[{}].key", name, i))
                        }
                        None => None,
                    };
                    let out = v.get("out");
                    let out = match out.map(|x| x.as_integer()) {
                        Some(Some(v)) => Some(Key::ControlChange(v as u8)),
                        Some(None) => {
                            return Err(anyhow::anyhow!("Type error at keys.{}[{}].out", name, i))
                        }
                        None => None,
                    };
                    let values = v
                        .get("values")
                        .ok_or_else(|| anyhow::anyhow!("Required: keys.{}[{}].values", name, i))?
                        .as_array()
                        .ok_or_else(|| {
                            anyhow::anyhow!("Type error at keys.{}[{}].values", name, i)
                        })?;
                    let values = values
                        .iter()
                        .map(|x| x.as_str().map(|x| x.to_owned()))
                        .collect::<Option<Vec<_>>>()
                        .ok_or_else(|| {
                            anyhow::anyhow!("Type error at keys.{}.[{}].values", name, i)
                        })?;
                    if let Some(key) = key {
                        state_in.define_input(
                            key,
                            InputConfig::Enum {
                                name: name.to_owned(),
                                values: values.clone(),
                            },
                        );
                    }
                    if let Some(out) = out {
                        state_out.define_output(OutputConfig::Enum {
                            name: name.to_owned(),
                            values,
                            out,
                        });
                    }
                }
            }
        }
    }
    Ok(())
}
