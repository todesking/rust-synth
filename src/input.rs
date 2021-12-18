use crate::WaveForm;
use anyhow::Result;

pub trait SimpleEnum
where
    Self: Sized,
{
    fn from_name(name: &str) -> Option<Self>;
    fn to_name(&self) -> &'static str;
}
impl SimpleEnum for WaveForm {
    fn from_name(name: &str) -> Option<Self> {
        match name {
            "Sine" => Some(WaveForm::Sine),
            "Triangle" => Some(WaveForm::Triangle),
            "Sawtooth" => Some(WaveForm::Sawtooth),
            "Square" => Some(WaveForm::Square),
            "Noise" => Some(WaveForm::Noise),
            _ => None,
        }
    }
    fn to_name(&self) -> &'static str {
        match self {
            WaveForm::Sine => "Sine",
            WaveForm::Triangle => "Triangle",
            WaveForm::Sawtooth => "Sawtooth",
            WaveForm::Square => "Square",
            WaveForm::Noise => "Noise",
        }
    }
}

pub enum ButtonMode {
    Toggle,
    Momentary,
}

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
pub struct StateInput<S> {
    state_definition: std::sync::Arc<StateDefinition<S>>,
    inputs: std::collections::HashMap<Key, InputConfig>,
}
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
}
impl<S> StateInput<S> {
    pub fn new(state_definition: std::sync::Arc<StateDefinition<S>>) -> StateInput<S> {
        StateInput {
            state_definition,
            inputs: std::collections::HashMap::new(),
        }
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

#[derive(PartialEq, Eq, Hash, Clone)]
pub enum Key {
    ControlChange(u8),
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

#[macro_export]
macro_rules! define_input_field_default {
    () => {
        ::std::default::Default::default()
    };
    ($expr:expr) => {
        $expr
    };
}
#[macro_export]
macro_rules! define_input {
    ($name:ident {
        $($field:ident : $ty:ty $(= $default_value:tt)?),*$(,)?
    }) => {
        #[derive(Clone, Debug)]
        pub struct $name {
            $(
                pub $field: $ty
            ),*
        }
        impl $name {
            pub fn new_state_definition() -> $crate::input::StateDefinition<Self> {
                let mut key_mapping = $crate::input::StateDefinition::<$name>::new();
                $(
                    $crate::input::DefineField::<$name, $ty>::define_field(
                        &mut key_mapping,
                        stringify!($field).to_owned(),
                        |input| &input.$field,
                        |input, value| input.$field = value
                    );
                )*
                key_mapping
            }
        }
        impl ::std::default::Default for $name {
            fn default() -> $name {
                $name {
                    $(
                        $field: $crate::define_input_field_default!($($default_value)?)
                    ),*
                }
            }
        }
    };
}
