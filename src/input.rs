use crate::SimpleEnum;
use anyhow::Result;

pub trait Input: Send + Sync + Clone + std::fmt::Debug
where
    Self: Sized,
{
    fn new_state_definition() -> StateDefinition<Self>;
}

#[derive(Debug)]
pub enum ButtonMode {
    Toggle,
    Momentary,
}

#[derive(Debug, PartialEq, Eq, Hash, Clone)]
pub enum Key {
    ControlChange(u8),
}

#[derive(Debug)]
pub enum FieldType {
    F32,
    Bool,
    Enum,
}

#[derive(Debug)]
pub enum InputConfig {
    F32 { name: String },
    Bool { name: String, mode: ButtonMode },
    Enum { name: String, values: Vec<String> },
}
impl InputConfig {
    fn name(&self) -> &str {
        match self {
            Self::F32 { name } => name,
            Self::Bool { name, .. } => name,
            Self::Enum { name, .. } => name,
        }
    }
}

#[derive(Debug)]
pub enum OutputConfig {
    Bool {
        name: String,
        out: Key,
    },
    Enum {
        name: String,
        out: Key,
        values: Vec<String>,
    },
}
impl OutputConfig {
    fn name(&self) -> &str {
        match self {
            OutputConfig::Bool { name, .. } => name,
            OutputConfig::Enum { name, .. } => name,
        }
    }
}

#[derive(Debug)]
pub struct StateDefinition<S> {
    accessors: std::collections::HashMap<String, FieldAccessor<S>>,
}
impl<S> Default for StateDefinition<S> {
    fn default() -> Self {
        Self {
            accessors: std::collections::HashMap::new(),
        }
    }
}
#[derive(Debug)]
pub struct StateInput<S> {
    state_definition: std::sync::Arc<StateDefinition<S>>,
    inputs: std::collections::HashMap<Key, InputConfig>,
}
#[derive(Debug)]
pub struct StateOutput<S> {
    state_definition: std::sync::Arc<StateDefinition<S>>,
    outputs: Vec<OutputConfig>,
}
impl<S> StateDefinition<S> {
    pub fn new() -> Self {
        Default::default()
    }
    pub fn into_io(self) -> (StateInput<S>, StateOutput<S>) {
        let sd = std::sync::Arc::new(self);
        (StateInput::new(sd.clone()), StateOutput::new(sd))
    }
    pub fn define_field(&mut self, name: String, accessor: FieldAccessor<S>) {
        self.accessors.insert(name, accessor);
    }
    pub fn assert_has_field(&self, name: &str) {
        if !self.accessors.contains_key(name) {
            panic!("Undefined field: {}", name);
        }
    }
    pub fn field(&self, name: &str) -> &FieldAccessor<S> {
        self.accessors
            .get(name)
            .unwrap_or_else(|| panic!("Undefined field: {}", name))
    }
    pub fn field_type(&self, name: &str) -> Option<FieldType> {
        self.accessors.get(name).map(|a| match a {
            FieldAccessor::Bool(..) => FieldType::Bool,
            FieldAccessor::F32(..) => FieldType::F32,
            FieldAccessor::Enum(..) => FieldType::Enum,
        })
    }
}
impl<S> StateInput<S> {
    pub fn new(state_definition: std::sync::Arc<StateDefinition<S>>) -> StateInput<S> {
        StateInput {
            state_definition,
            inputs: std::collections::HashMap::new(),
        }
    }
    pub fn field_type(&self, name: &str) -> Option<FieldType> {
        self.state_definition.field_type(name)
    }
    pub fn define_input(&mut self, key: Key, input: InputConfig) {
        self.state_definition.assert_has_field(input.name());
        self.inputs.insert(key, input);
    }
    pub fn update_state(&self, state: &mut S, key: Key, value: u8) {
        if let Some(input) = self.inputs.get(&key) {
            match input {
                InputConfig::Bool { name, mode } => match self.state_definition.field(name) {
                    FieldAccessor::Bool(get, set) => {
                        let pressed = 0x40 <= value;
                        match mode {
                            ButtonMode::Momentary => {
                                set(state, pressed);
                            }
                            ButtonMode::Toggle => {
                                let current = get(state);
                                if pressed {
                                    set(state, !current);
                                }
                            }
                        }
                    }
                    _ => {
                        panic!("assertion error: {}", name);
                    }
                },
                InputConfig::F32 { name } => match self.state_definition.field(name) {
                    FieldAccessor::F32(_, set) => {
                        let value = value as f32 / 127.0f32;
                        set(state, value);
                    }
                    _ => {
                        panic!("assertion error: {}", name);
                    }
                },
                InputConfig::Enum { name, values } => match self.state_definition.field(name) {
                    FieldAccessor::Enum(get, set) => {
                        let pressed = 0x40 <= value;
                        if pressed {
                            let current = get(state);
                            let mut index = 0;
                            for (i, v) in values.iter().enumerate() {
                                if v == current {
                                    index = (i + 1) % values.len();
                                    break;
                                }
                            }
                            set(state, &values[index]);
                        }
                    }
                    _ => {
                        panic!("assertion error: {}", name);
                    }
                },
            }
        }
    }
}
impl<S> StateOutput<S> {
    pub fn new(state_definition: std::sync::Arc<StateDefinition<S>>) -> StateOutput<S> {
        StateOutput {
            state_definition,
            outputs: Vec::new(),
        }
    }
    pub fn define_output(&mut self, output: OutputConfig) {
        self.state_definition.assert_has_field(output.name());
        self.outputs.push(output);
    }
    pub fn output<F: FnMut(&Key, bool) -> Result<()>>(&self, state: &S, mut f: F) -> Result<()> {
        for o in self.outputs.iter() {
            match o {
                OutputConfig::Bool { name, out } => match self.state_definition.field(name) {
                    FieldAccessor::Bool(get, _) => {
                        f(out, get(state))?;
                    }
                    _ => {
                        panic!("assertion error: {}", name);
                    }
                },
                OutputConfig::Enum { name, out, values } => {
                    match self.state_definition.field(name) {
                        FieldAccessor::Enum(get, _) => {
                            let s = get(state);
                            f(out, values.iter().any(|v| v == s))?;
                        }
                        _ => {
                            panic!("assertion error: {}", name);
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

type Get<S, T> = Box<dyn Fn(&S) -> T + Send + Sync>;
type Set<S, T> = Box<dyn Fn(&mut S, T) + Send + Sync>;
pub enum FieldAccessor<S> {
    F32(Get<S, f32>, Set<S, f32>),
    Bool(Get<S, bool>, Set<S, bool>),
    Enum(
        Get<S, &'static str>,
        Box<dyn Fn(&mut S, &str) + Send + Sync>,
    ),
}
impl<S> std::fmt::Debug for FieldAccessor<S> {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::result::Result<(), std::fmt::Error> {
        match self {
            FieldAccessor::F32(..) => fmt
                .debug_struct("FieldAccessor::F32")
                .finish_non_exhaustive(),
            FieldAccessor::Bool(..) => fmt
                .debug_struct("FieldAccessor::Bool")
                .finish_non_exhaustive(),
            FieldAccessor::Enum(..) => fmt
                .debug_struct("FieldAccessor::Enum")
                .finish_non_exhaustive(),
        }
    }
}

pub trait DefineField<S, T> {
    fn define_field<
        Get: Fn(&S) -> &T + 'static + Send + Sync,
        Set: Fn(&mut S, T) + 'static + Send + Sync,
    >(
        &mut self,
        name: String,
        get: Get,
        set: Set,
    );
}
impl<S> DefineField<S, f32> for StateDefinition<S> {
    fn define_field<
        Get: Fn(&S) -> &f32 + 'static + Send + Sync,
        Set: Fn(&mut S, f32) + 'static + Send + Sync,
    >(
        &mut self,
        name: String,
        get: Get,
        set: Set,
    ) {
        self.define_field(
            name,
            FieldAccessor::F32(
                Box::new(move |input| *get(input)),
                Box::new(move |input, value| set(input, value)),
            ),
        )
    }
}
impl<S> DefineField<S, bool> for StateDefinition<S> {
    fn define_field<
        Get: Fn(&S) -> &bool + 'static + Send + Sync,
        Set: Fn(&mut S, bool) + 'static + Send + Sync,
    >(
        &mut self,
        name: String,
        get: Get,
        set: Set,
    ) {
        self.define_field(
            name,
            FieldAccessor::Bool(
                Box::new(move |input| *get(input)),
                Box::new(move |input, value| set(input, value)),
            ),
        )
    }
}
impl<S, E: SimpleEnum> DefineField<S, E> for StateDefinition<S> {
    fn define_field<
        Get: Fn(&S) -> &E + 'static + Send + Sync,
        Set: Fn(&mut S, E) + 'static + Send + Sync,
    >(
        &mut self,
        name: String,
        get: Get,
        set: Set,
    ) {
        self.define_field(
            name,
            FieldAccessor::Enum(
                Box::new(move |input| get(input).to_name()),
                Box::new(move |input, value| {
                    if let Some(value) = E::from_name(value) {
                        set(input, value)
                    }
                }),
            ),
        )
    }
}
