use anyhow::{Context as _, Result};
use collections::HashMap;
pub use gpui_macros::Action;
pub use no_action::{NoAction, Unbind, is_no_action, is_unbind};
use serde_json::json;
use std::{
    any::{Any, TypeId},
    fmt::Display,
    time::{Duration, Instant},
};

/// Identifies the route that initiated an action dispatch.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ActionInvocationSource {
    /// The action was invoked by application code.
    #[default]
    Programmatic,
    /// The action was resolved from a keyboard binding.
    Keyboard,
    /// The action was invoked by pointer interaction.
    Pointer,
    /// The action was invoked from a menu.
    Menu,
    /// The action was invoked from a command palette.
    Palette,
    /// The action was invoked through an accessibility surface.
    Accessibility,
}

/// Immutable metadata for one action dispatch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActionInvocation {
    id: u64,
    action_name: &'static str,
    source: ActionInvocationSource,
    parent_id: Option<u64>,
    window_id: Option<u64>,
    target: Option<usize>,
    context: Vec<String>,
    started_at: Instant,
}

impl ActionInvocation {
    /// Creates invocation metadata for an action dispatch.
    pub fn new(
        id: u64,
        action_name: &'static str,
        source: ActionInvocationSource,
        parent_id: Option<u64>,
        window_id: Option<u64>,
        target: Option<usize>,
        context: Vec<String>,
    ) -> Self {
        Self {
            id,
            action_name,
            source,
            parent_id,
            window_id,
            target,
            context,
            started_at: Instant::now(),
        }
    }

    /// Returns the monotonic dispatch identifier.
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Returns the stable GPUI action name without serializing action payloads.
    pub fn action_name(&self) -> &'static str {
        self.action_name
    }

    /// Returns the invocation route.
    pub fn source(&self) -> ActionInvocationSource {
        self.source
    }

    /// Returns the parent dispatch identifier for nested actions.
    pub fn parent_id(&self) -> Option<u64> {
        self.parent_id
    }

    /// Returns the window identifier captured at dispatch entry.
    pub fn window_id(&self) -> Option<u64> {
        self.window_id
    }

    /// Returns the dispatch-tree node index captured at invocation time.
    pub fn target(&self) -> Option<usize> {
        self.target
    }

    /// Returns the key-context names captured at invocation time.
    pub fn context(&self) -> &[String] {
        &self.context
    }

    /// Returns the monotonic instant at which dispatch metadata was created.
    pub fn started_at(&self) -> Instant {
        self.started_at
    }
}

/// The terminal state of an action dispatch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActionDispatchOutcome {
    /// An action listener handled the action.
    Handled,
    /// No action listener handled the action.
    Unhandled,
    /// An action listener reported a failure.
    Failed,
    /// An action listener cancelled the action.
    Cancelled,
}

/// Converts handler return values into an action dispatch outcome.
pub trait IntoActionDispatchOutcome {
    /// Returns the explicit terminal outcome for the handler invocation.
    fn into_action_dispatch_outcome(self) -> ActionDispatchOutcome;
}

impl IntoActionDispatchOutcome for () {
    fn into_action_dispatch_outcome(self) -> ActionDispatchOutcome {
        ActionDispatchOutcome::Handled
    }
}

impl IntoActionDispatchOutcome for ActionDispatchOutcome {
    fn into_action_dispatch_outcome(self) -> ActionDispatchOutcome {
        self
    }
}

/// The immutable terminal report for an action dispatch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActionDispatchReport {
    invocation: ActionInvocation,
    outcome: ActionDispatchOutcome,
    handled_count: usize,
    duration: Duration,
}

impl ActionDispatchReport {
    /// Creates the terminal report for an action dispatch.
    pub fn new(
        invocation: ActionInvocation,
        outcome: ActionDispatchOutcome,
        handled_count: usize,
        duration: Duration,
    ) -> Self {
        Self {
            invocation,
            outcome,
            handled_count,
            duration,
        }
    }

    /// Returns immutable metadata for this dispatch.
    pub fn invocation(&self) -> &ActionInvocation {
        &self.invocation
    }

    /// Returns the terminal dispatch outcome.
    pub fn outcome(&self) -> ActionDispatchOutcome {
        self.outcome
    }

    /// Returns the number of listeners that handled the action.
    pub fn handled_count(&self) -> usize {
        self.handled_count
    }

    /// Returns the elapsed synchronous dispatch duration.
    pub fn duration(&self) -> Duration {
        self.duration
    }
}

/// A lifecycle event emitted before and after an action dispatch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ActionDispatchEvent {
    /// The dispatch has started.
    Began(ActionInvocation),
    /// The dispatch reached one terminal outcome.
    Ended(ActionDispatchReport),
}

/// Defines and registers unit structs that can be used as actions. For more complex data types, derive `Action`.
///
/// For example:
///
/// ```
/// use gpux::actions;
/// actions!(editor, [MoveUp, MoveDown, MoveLeft, MoveRight, Newline]);
/// ```
///
/// This will create actions with names like `editor::MoveUp`, `editor::MoveDown`, etc.
///
/// The namespace argument `editor` can also be omitted, though it is required for Zed actions.
#[macro_export]
macro_rules! actions {
    ($namespace:path, [ $( $(#[$attr:meta])* $name:ident),* $(,)? ]) => {
        $(
            #[derive(::std::clone::Clone, ::std::cmp::PartialEq, ::std::default::Default, ::std::fmt::Debug, gpux::Action)]
            #[action(namespace = $namespace)]
            $(#[$attr])*
            pub struct $name;
        )*
    };
    ([ $( $(#[$attr:meta])* $name:ident),* $(,)? ]) => {
        $(
            #[derive(::std::clone::Clone, ::std::cmp::PartialEq, ::std::default::Default, ::std::fmt::Debug, gpux::Action)]
            $(#[$attr])*
            pub struct $name;
        )*
    };
}

/// Actions are used to implement keyboard-driven UI. When you declare an action, you can bind keys
/// to the action in the keymap and listeners for that action in the element tree.
///
/// To declare a list of simple actions, you can use the actions! macro, which defines a simple unit
/// struct action for each listed action name in the given namespace.
///
/// ```
/// use gpux::actions;
/// actions!(editor, [MoveUp, MoveDown, MoveLeft, MoveRight, Newline]);
/// ```
///
/// Registering the actions with the same name will result in a panic during  `App` creation.
///
/// # Derive Macro
///
/// More complex data types can also be actions, by using the derive macro for `Action`:
///
/// ```
/// use gpux::Action;
/// #[derive(Clone, PartialEq, serde::Deserialize, schemars::JsonSchema, Action)]
/// #[action(namespace = editor)]
/// pub struct SelectNext {
///     pub replace_newest: bool,
/// }
/// ```
///
/// The derive macro for `Action` requires that the type implement `Clone` and `PartialEq`. It also
/// requires `serde::Deserialize` and `schemars::JsonSchema` unless `#[action(no_json)]` is
/// specified. In Zed these trait impls are used to load keymaps from JSON.
///
/// Multiple arguments separated by commas may be specified in `#[action(...)]`:
///
/// - `namespace = some_namespace` sets the namespace. In Zed this is required.
///
/// - `name = "ActionName"` overrides the action's name. This must not contain `::`.
///
/// - `no_json` causes the `build` method to always error and `action_json_schema` to return `None`,
///   and allows actions not implement `serde::Serialize` and `schemars::JsonSchema`.
///
/// - `no_register` skips registering the action. This is useful for implementing the `Action` trait
///   while not supporting invocation by name or JSON deserialization.
///
/// - `deprecated_aliases = ["editor::SomeAction"]` specifies deprecated old names for the action.
///   These action names should *not* correspond to any actions that are registered. These old names
///   can then still be used to refer to invoke this action. In Zed, the keymap JSON schema will
///   accept these old names and provide warnings.
///
/// - `deprecated = "Message about why this action is deprecation"` specifies a deprecation message.
///   In Zed, the keymap JSON schema will cause this to be displayed as a warning.
///
/// # Manual Implementation
///
/// If you want to control the behavior of the action trait manually, you can use the lower-level
/// `#[register_action]` macro, which only generates the code needed to register your action before
/// `main`.
///
/// ```
/// use gpux::{SharedString, register_action};
/// #[derive(Clone, PartialEq, Eq, serde::Deserialize, schemars::JsonSchema)]
/// pub struct Paste {
///     pub content: SharedString,
/// }
///
/// impl gpux::Action for Paste {
///     # fn boxed_clone(&self) -> Box<dyn gpux::Action> { unimplemented!()}
///     # fn partial_eq(&self, other: &dyn gpux::Action) -> bool { unimplemented!() }
///     # fn name(&self) -> &'static str { "Paste" }
///     # fn name_for_type() -> &'static str { "Paste" }
///     # fn build(value: serde_json::Value) -> anyhow::Result<Box<dyn gpux::Action>> {
///     #     unimplemented!()
///     # }
/// }
///
/// register_action!(Paste);
/// ```
pub trait Action: Any + Send {
    /// Clone the action into a new box
    fn boxed_clone(&self) -> Box<dyn Action>;

    /// Do a partial equality check on this action and the other
    fn partial_eq(&self, action: &dyn Action) -> bool;

    /// Get the name of this action, for displaying in UI
    fn name(&self) -> &'static str;

    /// Get the name of this action type (static)
    fn name_for_type() -> &'static str
    where
        Self: Sized;

    /// Build this action from a JSON value. This is used to construct actions from the keymap.
    /// A value of `{}` will be passed for actions that don't have any parameters.
    fn build(value: serde_json::Value) -> Result<Box<dyn Action>>
    where
        Self: Sized;

    /// Optional JSON schema for the action's input data.
    fn action_json_schema(_: &mut schemars::SchemaGenerator) -> Option<schemars::Schema>
    where
        Self: Sized,
    {
        None
    }

    /// A list of alternate, deprecated names for this action. These names can still be used to
    /// invoke the action. In Zed, the keymap JSON schema will accept these old names and provide
    /// warnings.
    fn deprecated_aliases() -> &'static [&'static str]
    where
        Self: Sized,
    {
        &[]
    }

    /// Returns the deprecation message for this action, if any. In Zed, the keymap JSON schema will
    /// cause this to be displayed as a warning.
    fn deprecation_message() -> Option<&'static str>
    where
        Self: Sized,
    {
        None
    }

    /// The documentation for this action, if any. When using the derive macro for actions
    /// this will be automatically generated from the doc comments on the action struct.
    fn documentation() -> Option<&'static str>
    where
        Self: Sized,
    {
        None
    }
}

impl std::fmt::Debug for dyn Action {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("dyn Action")
            .field("name", &self.name())
            .finish()
    }
}

impl dyn Action {
    /// Type-erase Action type.
    pub fn as_any(&self) -> &dyn Any {
        self as &dyn Any
    }
}

/// Error type for `Keystroke::parse`. This is used instead of `anyhow::Error` so that Zed can use
/// markdown to display it.
#[derive(Debug)]
pub enum ActionBuildError {
    /// Indicates that an action with this name has not been registered.
    NotFound {
        /// Name of the action that was not found.
        name: String,
    },
    /// Indicates that an error occurred while building the action, typically a JSON deserialization
    /// error.
    BuildError {
        /// Name of the action that was attempting to be built.
        name: String,
        /// Error that occurred while building the action.
        error: anyhow::Error,
    },
}

impl std::error::Error for ActionBuildError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ActionBuildError::NotFound { .. } => None,
            ActionBuildError::BuildError { error, .. } => error.source(),
        }
    }
}

impl Display for ActionBuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ActionBuildError::NotFound { name } => {
                write!(f, "Didn't find an action named \"{name}\"")
            }
            ActionBuildError::BuildError { name, error } => {
                write!(f, "Error while building action \"{name}\": {error}")
            }
        }
    }
}

type ActionBuilder = fn(json: serde_json::Value) -> anyhow::Result<Box<dyn Action>>;

pub(crate) struct ActionRegistry {
    by_name: HashMap<&'static str, ActionData>,
    names_by_type_id: HashMap<TypeId, &'static str>,
    all_names: Vec<&'static str>, // So we can return a static slice.
    deprecated_aliases: HashMap<&'static str, &'static str>, // deprecated name -> preferred name
    deprecation_messages: HashMap<&'static str, &'static str>, // action name -> deprecation message
    documentation: HashMap<&'static str, &'static str>, // action name -> documentation
}

impl Default for ActionRegistry {
    fn default() -> Self {
        let mut this = ActionRegistry {
            by_name: Default::default(),
            names_by_type_id: Default::default(),
            documentation: Default::default(),
            all_names: Default::default(),
            deprecated_aliases: Default::default(),
            deprecation_messages: Default::default(),
        };

        this.load_actions();

        this
    }
}

struct ActionData {
    pub build: ActionBuilder,
    pub json_schema: fn(&mut schemars::SchemaGenerator) -> Option<schemars::Schema>,
}

/// This type must be public so that our macros can build it in other crates.
/// But this is an implementation detail and should not be used directly.
#[doc(hidden)]
pub struct MacroActionBuilder(pub fn() -> MacroActionData);

/// This type must be public so that our macros can build it in other crates.
/// But this is an implementation detail and should not be used directly.
#[doc(hidden)]
pub struct MacroActionData {
    pub name: &'static str,
    pub type_id: TypeId,
    pub build: ActionBuilder,
    pub json_schema: fn(&mut schemars::SchemaGenerator) -> Option<schemars::Schema>,
    pub deprecated_aliases: &'static [&'static str],
    pub deprecation_message: Option<&'static str>,
    pub documentation: Option<&'static str>,
}

inventory::collect!(MacroActionBuilder);

impl ActionRegistry {
    /// Load all registered actions into the registry.
    pub(crate) fn load_actions(&mut self) {
        for builder in inventory::iter::<MacroActionBuilder> {
            let action = builder.0();
            self.insert_action(action);
        }
    }

    fn insert_action(&mut self, action: MacroActionData) {
        let name = action.name;
        if self.by_name.contains_key(name) {
            panic!(
                "Action with name `{name}` already registered \
                (might be registered in `#[action(deprecated_aliases = [...])]`."
            );
        }
        self.by_name.insert(
            name,
            ActionData {
                build: action.build,
                json_schema: action.json_schema,
            },
        );
        for &alias in action.deprecated_aliases {
            if self.by_name.contains_key(alias) {
                panic!(
                    "Action with name `{alias}` already registered. \
                    `{alias}` is specified in `#[action(deprecated_aliases = [...])]` for action `{name}`."
                );
            }
            self.by_name.insert(
                alias,
                ActionData {
                    build: action.build,
                    json_schema: action.json_schema,
                },
            );
            self.deprecated_aliases.insert(alias, name);
            self.all_names.push(alias);
        }
        self.names_by_type_id.insert(action.type_id, name);
        self.all_names.push(name);
        if let Some(deprecation_msg) = action.deprecation_message {
            self.deprecation_messages.insert(name, deprecation_msg);
        }
        if let Some(documentation) = action.documentation {
            self.documentation.insert(name, documentation);
        }
    }

    /// Construct an action based on its name and optional JSON parameters sourced from the keymap.
    pub fn build_action_type(&self, type_id: &TypeId) -> Result<Box<dyn Action>> {
        let name = self
            .names_by_type_id
            .get(type_id)
            .with_context(|| format!("no action type registered for {type_id:?}"))?;

        Ok(self.build_action(name, None)?)
    }

    /// Construct an action based on its name and optional JSON parameters sourced from the keymap.
    pub fn build_action(
        &self,
        name: &str,
        params: Option<serde_json::Value>,
    ) -> std::result::Result<Box<dyn Action>, ActionBuildError> {
        let build_action = self
            .by_name
            .get(name)
            .ok_or_else(|| ActionBuildError::NotFound {
                name: name.to_owned(),
            })?
            .build;
        (build_action)(params.unwrap_or_else(|| json!({}))).map_err(|e| {
            ActionBuildError::BuildError {
                name: name.to_owned(),
                error: e,
            }
        })
    }

    pub fn all_action_names(&self) -> &[&'static str] {
        self.all_names.as_slice()
    }

    pub fn action_schemas(
        &self,
        generator: &mut schemars::SchemaGenerator,
    ) -> Vec<(&'static str, Option<schemars::Schema>)> {
        // Use the order from all_names so that the resulting schema has sensible order.
        self.all_names
            .iter()
            .map(|name| {
                let action_data = self
                    .by_name
                    .get(name)
                    .expect("All actions in all_names should be registered");
                (*name, (action_data.json_schema)(generator))
            })
            .collect::<Vec<_>>()
    }

    pub fn action_schema_by_name(
        &self,
        name: &str,
        generator: &mut schemars::SchemaGenerator,
    ) -> Option<Option<schemars::Schema>> {
        self.by_name
            .get(name)
            .map(|action_data| (action_data.json_schema)(generator))
    }

    pub fn deprecated_aliases(&self) -> &HashMap<&'static str, &'static str> {
        &self.deprecated_aliases
    }

    pub fn deprecation_messages(&self) -> &HashMap<&'static str, &'static str> {
        &self.deprecation_messages
    }

    pub fn documentation(&self) -> &HashMap<&'static str, &'static str> {
        &self.documentation
    }
}

/// Generate a list of all the registered actions.
/// Useful for transforming the list of available actions into a
/// format suited for static analysis such as in validating keymaps, or
/// generating documentation.
pub fn generate_list_of_all_registered_actions() -> impl Iterator<Item = MacroActionData> {
    inventory::iter::<MacroActionBuilder>
        .into_iter()
        .map(|builder| builder.0())
}

mod no_action {
    use crate as gpux;
    use schemars::JsonSchema;
    use serde::Deserialize;

    actions!(
        zed,
        [
            /// Action with special handling which unbinds the keybinding this is associated with,
            /// if it is the highest precedence match.
            NoAction
        ]
    );

    /// Action with special handling which unbinds later bindings for the same keystrokes when they
    /// dispatch the named action, regardless of that action's context.
    ///
    /// In keymap JSON this is written as:
    ///
    /// `["zed::Unbind", "editor::NewLine"]`
    #[derive(Clone, Debug, PartialEq, Deserialize, JsonSchema, gpux::Action)]
    #[action(namespace = zed)]
    pub struct Unbind(pub gpux::SharedString);

    /// Returns whether or not this action represents a removed key binding.
    pub fn is_no_action(action: &dyn gpux::Action) -> bool {
        action.as_any().is::<NoAction>()
    }

    /// Returns whether or not this action represents an unbind marker.
    pub fn is_unbind(action: &dyn gpux::Action) -> bool {
        action.as_any().is::<Unbind>()
    }
}
