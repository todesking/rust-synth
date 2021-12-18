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
        impl $crate::input::Input for $name {
            fn new_state_definition() -> $crate::input::StateDefinition<Self> {
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

#[macro_export]
macro_rules! define_rack_field_value {
    ($param_rack:ident, $ty_rack:ident, $param_input:ident, $ty_input:ident, { $($stmt:stmt)* }) => {
        ::std::boxed::Box::new(
            #[allow(unused_variables)]
            #[allow(redundant_semicolons)]
            |$param_rack: &$ty_rack, $param_input: &$ty_input| { $($stmt)* },
        )
    };
    ($param_rack:ident, $ty_rack:ident, $param_input:ident, $ty_input:ident, $expr:expr) => {
        $expr
    };
}

#[macro_export]
macro_rules! define_rack {
    ($rack_name:ident : Rack<$input:ident>($param_rack:ident, $param_input:ident) {$(
        $mod_name:ident : $mod_type:ident {$(
            $field_name:ident : $field_value:tt
        ),*$(,)?}
    ),*$(,)?}) => {
        pub struct $rack_name {
            $(pub $mod_name: ::std::cell::RefCell<$mod_type<$rack_name>> ),*
        }
        impl $rack_name {
            pub fn new() -> $rack_name {
                $rack_name {
                    $($mod_name: ::std::cell::RefCell::new(
                        $mod_type {
                            $($field_name: $crate::define_rack_field_value!($param_rack, $rack_name, $param_input, $input, $field_value)),*
                            ,..::std::default::Default::default()
                        }
                    )),*
                }
            }
        }
        impl $crate::module::Rack for $rack_name {
            type Input = $input;
            fn new_input() -> Self::Input {
                ::std::default::Default::default()
            }
            fn update(&self, input: &$input) {
                $({
                    let mut module = ::std::cell::RefCell::borrow_mut(&self.$mod_name);
                    $crate::module::Module::update(&mut *module, self, input);
                })*
            }
        }
    };
}
