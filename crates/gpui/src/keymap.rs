mod binding;
mod context;

pub use binding::*;
pub use context::*;

use crate::{Action, AsKeystroke, Keystroke, SharedString, Unbind, is_no_action, is_unbind};
use collections::{HashMap, HashSet};
use smallvec::SmallVec;
use std::{any::TypeId, fmt, rc::Rc};

/// An opaque identifier of which version of the keymap is currently active.
/// The keymap's version is changed whenever a layer snapshot is replaced.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub struct KeymapVersion(usize);

/// A stable identity for a named input-map layer.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct InputLayerId(SharedString);

impl InputLayerId {
    /// Creates a layer identity from a stable application-owned name.
    pub fn new(id: impl Into<SharedString>) -> Self {
        Self(id.into())
    }
}

impl fmt::Display for InputLayerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// An immutable, ordered set of key bindings owned by one input layer.
#[derive(Clone)]
pub struct KeymapLayer {
    id: InputLayerId,
    priority: i32,
    bindings: Rc<[KeyBinding]>,
}

impl KeymapLayer {
    /// Creates a layer. Higher priorities win after context specificity is resolved.
    pub fn new(
        id: InputLayerId,
        priority: i32,
        bindings: impl IntoIterator<Item = KeyBinding>,
    ) -> Self {
        Self {
            id,
            priority,
            bindings: bindings.into_iter().collect(),
        }
    }

    /// Returns the layer identity.
    pub fn id(&self) -> &InputLayerId {
        &self.id
    }

    /// Returns the precedence priority for this layer.
    pub fn priority(&self) -> i32 {
        self.priority
    }

    /// Returns the layer's bindings in their declared rule order.
    pub fn bindings(&self) -> &[KeyBinding] {
        &self.bindings
    }
}

/// A typed error produced before an input-map snapshot is swapped.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InputMapError {
    /// A candidate snapshot contains the same layer identity more than once.
    DuplicateLayerId(InputLayerId),
}

impl fmt::Display for InputMapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateLayerId(id) => write!(f, "duplicate input layer id: {id}"),
        }
    }
}

impl std::error::Error for InputMapError {}

/// An immutable keymap layer snapshot retained by in-progress input sequences.
pub struct KeymapSnapshot {
    layers: Vec<KeymapLayer>,
    bindings: Vec<KeyBinding>,
    binding_indices_by_action_id: HashMap<TypeId, SmallVec<[usize; 3]>>,
    disabled_binding_indices: Vec<usize>,
}

impl KeymapSnapshot {
    fn new(mut layers: Vec<KeymapLayer>) -> Result<Self, InputMapError> {
        layers.sort_by(|left, right| {
            left.priority
                .cmp(&right.priority)
                .then_with(|| left.id.cmp(&right.id))
        });

        let mut bindings = Vec::new();
        let mut binding_indices_by_action_id: HashMap<TypeId, SmallVec<[usize; 3]>> =
            HashMap::default();
        let mut disabled_binding_indices = Vec::new();
        let mut previous_id = None;
        for layer in &layers {
            if previous_id == Some(&layer.id) {
                return Err(InputMapError::DuplicateLayerId(layer.id.clone()));
            }
            previous_id = Some(&layer.id);
            for binding in layer.bindings() {
                let binding_index = bindings.len();
                if is_no_action(&*binding.action) || is_unbind(&*binding.action) {
                    disabled_binding_indices.push(binding_index);
                } else {
                    binding_indices_by_action_id
                        .entry(binding.action().as_any().type_id())
                        .or_default()
                        .push(binding_index);
                }
                bindings.push(binding.clone());
            }
        }

        Ok(Self {
            layers,
            bindings,
            binding_indices_by_action_id,
            disabled_binding_indices,
        })
    }
}

/// A collection of key bindings resolved from an immutable ordered layer snapshot.
pub struct Keymap {
    snapshot: Rc<KeymapSnapshot>,
    version: KeymapVersion,
}

impl Default for Keymap {
    fn default() -> Self {
        Self {
            snapshot: Rc::new(KeymapSnapshot::new(Vec::new()).expect("empty layers are valid")),
            version: KeymapVersion::default(),
        }
    }
}

/// Index of a binding within a keymap.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct BindingIndex(usize);

fn disabled_binding_matches_context(disabled_binding: &KeyBinding, binding: &KeyBinding) -> bool {
    match (
        &disabled_binding.context_predicate,
        &binding.context_predicate,
    ) {
        (None, _) => true,
        (Some(_), None) => false,
        (Some(disabled_predicate), Some(predicate)) => disabled_predicate.is_superset(predicate),
    }
}

fn binding_is_unbound(disabled_binding: &KeyBinding, binding: &KeyBinding) -> bool {
    disabled_binding.keystrokes == binding.keystrokes
        && disabled_binding
            .action()
            .as_any()
            .downcast_ref::<Unbind>()
            .is_some_and(|unbind| unbind.0.as_ref() == binding.action.name())
}

impl Keymap {
    /// Creates a keymap from one complete, validated layer snapshot.
    pub fn from_layers(
        layers: impl IntoIterator<Item = KeymapLayer>,
    ) -> Result<Self, InputMapError> {
        Ok(Self {
            snapshot: Rc::new(KeymapSnapshot::new(layers.into_iter().collect())?),
            version: KeymapVersion::default(),
        })
    }

    /// Get the current version of the keymap.
    pub fn version(&self) -> KeymapVersion {
        self.version
    }

    /// Returns the immutable snapshot used by one complete input dispatch.
    /// Captures the immutable map revision used for one complete input sequence.
    pub fn snapshot(&self) -> Rc<KeymapSnapshot> {
        self.snapshot.clone()
    }

    /// Resolves input against a previously captured immutable snapshot.
    pub fn bindings_for_input_at(
        &self,
        snapshot: &Rc<KeymapSnapshot>,
        input: &[impl AsKeystroke],
        context_stack: &[KeyContext],
    ) -> (SmallVec<[KeyBinding; 1]>, bool) {
        Self {
            snapshot: snapshot.clone(),
            version: self.version,
        }
        .bindings_for_input(input, context_stack)
    }

    /// Atomically replaces one named layer while retaining all unrelated layers.
    pub fn set_layer(&mut self, layer: KeymapLayer) -> Result<Option<KeymapLayer>, InputMapError> {
        let mut layers = self.snapshot.layers.clone();
        let previous = layers
            .iter()
            .position(|existing| existing.id == layer.id)
            .map(|index| std::mem::replace(&mut layers[index], layer.clone()));
        if previous.is_none() {
            layers.push(layer);
        }
        self.snapshot = Rc::new(KeymapSnapshot::new(layers)?);
        self.version.0 += 1;
        Ok(previous)
    }

    /// Removes one named layer. Missing layers are a typed no-op.
    pub fn remove_layer(&mut self, id: &InputLayerId) -> Option<KeymapLayer> {
        let mut layers = self.snapshot.layers.clone();
        let index = layers.iter().position(|layer| layer.id == *id)?;
        let removed = layers.remove(index);
        self.snapshot = Rc::new(KeymapSnapshot::new(layers).expect("removing a layer stays valid"));
        self.version.0 += 1;
        Some(removed)
    }

    /// Returns the installed layer with this identity.
    pub fn layer(&self, id: &InputLayerId) -> Option<&KeymapLayer> {
        self.snapshot.layers.iter().find(|layer| layer.id == *id)
    }

    /// Iterates over the installed layers in resolution order.
    pub fn layers(&self) -> impl ExactSizeIterator<Item = &KeymapLayer> {
        self.snapshot.layers.iter()
    }

    /// Iterate over all bindings, in the order they were added.
    pub fn bindings(&self) -> impl DoubleEndedIterator<Item = &KeyBinding> + ExactSizeIterator {
        self.snapshot.bindings.iter()
    }

    /// Iterate over all bindings for the given action, in the order they were added. For display,
    /// the last binding should take precedence.
    pub fn bindings_for_action<'a>(
        &'a self,
        action: &'a dyn Action,
    ) -> impl 'a + DoubleEndedIterator<Item = &'a KeyBinding> {
        let action_id = action.type_id();
        let binding_indices = self
            .snapshot
            .binding_indices_by_action_id
            .get(&action_id)
            .map_or(&[] as _, SmallVec::as_slice)
            .iter();

        binding_indices.filter_map(|ix| {
            let binding = &self.snapshot.bindings[*ix];
            if !binding.action().partial_eq(action) {
                return None;
            }

            for disabled_ix in &self.snapshot.disabled_binding_indices {
                if disabled_ix > ix {
                    let disabled_binding = &self.snapshot.bindings[*disabled_ix];
                    if disabled_binding.keystrokes != binding.keystrokes {
                        continue;
                    }

                    if is_no_action(&*disabled_binding.action) {
                        if disabled_binding_matches_context(disabled_binding, binding) {
                            return None;
                        }
                    } else if is_unbind(&*disabled_binding.action)
                        && disabled_binding_matches_context(disabled_binding, binding)
                        && binding_is_unbound(disabled_binding, binding)
                    {
                        return None;
                    }
                }
            }

            Some(binding)
        })
    }

    /// Returns all bindings that might match the input without checking context. The bindings
    /// returned in precedence order (reverse of the order they were added to the keymap).
    pub fn all_bindings_for_input(&self, input: &[Keystroke]) -> Vec<KeyBinding> {
        self.bindings()
            .rev()
            .filter(|binding| {
                binding
                    .match_keystrokes(input)
                    .is_some_and(|pending| !pending)
            })
            .cloned()
            .collect()
    }

    /// Returns a list of bindings that match the given input, and a boolean indicating whether or
    /// not more bindings might match if the input was longer. Bindings are returned in precedence
    /// order (higher precedence first, reverse of the order they were added to the keymap).
    ///
    /// Precedence is defined by the depth in the tree (matches on the Editor take precedence over
    /// matches on the Pane, then the Workspace, etc.). Bindings with no context are treated as the
    /// same as the deepest context.
    ///
    /// In the case of multiple bindings at the same depth, the ones added to the keymap later take
    /// precedence. User bindings are added after built-in bindings so that they take precedence.
    ///
    /// If a binding has been disabled with `"x": null` it will not be returned. Disabled bindings
    /// are evaluated with the same precedence rules so you can disable a rule in a given context
    /// only. A disabled binding only suppresses bindings from sources with equal or weaker
    /// precedence: a base keymap null hides default bindings, but user bindings still apply.
    pub fn bindings_for_input(
        &self,
        input: &[impl AsKeystroke],
        context_stack: &[KeyContext],
    ) -> (SmallVec<[KeyBinding; 1]>, bool) {
        let mut matched_bindings = SmallVec::<[(usize, BindingIndex, &KeyBinding); 1]>::new();
        let mut pending_bindings = SmallVec::<[(BindingIndex, &KeyBinding); 1]>::new();

        for (ix, binding) in self.bindings().enumerate().rev() {
            let Some(depth) = self.binding_enabled(binding, context_stack) else {
                continue;
            };
            let Some(pending) = binding.match_keystrokes(input) else {
                continue;
            };

            if !pending {
                matched_bindings.push((depth, BindingIndex(ix), binding));
            } else {
                pending_bindings.push((BindingIndex(ix), binding));
            }
        }

        matched_bindings.sort_by(|(depth_a, ix_a, _), (depth_b, ix_b, _)| {
            depth_b.cmp(depth_a).then(ix_b.cmp(ix_a))
        });

        let mut bindings: SmallVec<[_; 1]> = SmallVec::new();
        let mut first_binding_index = None;
        let mut unbound_bindings: Vec<&KeyBinding> = Vec::new();
        // A `NoAction` binding suppresses out-ranked bindings from sources with
        // equal or weaker precedence, while bindings from stronger sources (a
        // smaller meta, e.g. a user binding vs a base keymap null) still apply.
        // Bindings without a meta are treated as user bindings.
        let mut no_action_meta: Option<u32> = None;

        for (_, ix, binding) in matched_bindings {
            let meta = binding.meta.map_or(0, |meta| meta.0);
            if is_no_action(&*binding.action) {
                no_action_meta = Some(no_action_meta.map_or(meta, |existing| existing.min(meta)));
                continue;
            }

            if no_action_meta.is_some_and(|no_action_meta| meta >= no_action_meta) {
                continue;
            }

            if is_unbind(&*binding.action) {
                unbound_bindings.push(binding);
                continue;
            }

            if unbound_bindings
                .iter()
                .any(|disabled_binding| binding_is_unbound(disabled_binding, binding))
            {
                continue;
            }

            bindings.push(binding.clone());
            first_binding_index.get_or_insert(ix);
        }

        let mut pending = HashSet::default();
        for (ix, binding) in pending_bindings.into_iter().rev() {
            if let Some(binding_ix) = first_binding_index
                && binding_ix > ix
            {
                continue;
            }
            if is_no_action(&*binding.action) || is_unbind(&*binding.action) {
                pending.remove(&&binding.keystrokes);
                continue;
            }
            pending.insert(&binding.keystrokes);
        }

        (bindings, !pending.is_empty())
    }
    /// Check if the given binding is enabled, given a certain key context.
    /// Returns the deepest depth at which the binding matches, or None if it doesn't match.
    fn binding_enabled(&self, binding: &KeyBinding, contexts: &[KeyContext]) -> Option<usize> {
        if let Some(predicate) = &binding.context_predicate {
            predicate.depth_of(contexts)
        } else {
            Some(contexts.len())
        }
    }

    /// Find the bindings that can follow the current input sequence.
    pub fn possible_next_bindings_for_input(
        &self,
        input: &[Keystroke],
        context_stack: &[KeyContext],
    ) -> Vec<KeyBinding> {
        let mut bindings = self
            .bindings()
            .enumerate()
            .rev()
            .filter_map(|(ix, binding)| {
                let depth = self.binding_enabled(binding, context_stack)?;
                let pending = binding.match_keystrokes(input);
                match pending {
                    None => None,
                    Some(is_pending) => {
                        if !is_pending
                            || is_no_action(&*binding.action)
                            || is_unbind(&*binding.action)
                        {
                            return None;
                        }
                        Some((depth, BindingIndex(ix), binding))
                    }
                }
            })
            .collect::<Vec<_>>();

        bindings.sort_by(|(depth_a, ix_a, _), (depth_b, ix_b, _)| {
            depth_b.cmp(depth_a).then(ix_b.cmp(ix_a))
        });

        bindings
            .into_iter()
            .map(|(_, _, binding)| binding.clone())
            .collect::<Vec<_>>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate as gpux;
    use gpux::{NoAction, Unbind};

    actions!(
        test_only,
        [ActionAlpha, ActionBeta, ActionGamma, ActionDelta,]
    );

    fn test_keymap(bindings: impl IntoIterator<Item = KeyBinding>) -> Keymap {
        Keymap::from_layers([KeymapLayer::new(InputLayerId::new("test"), 0, bindings)]).unwrap()
    }

    #[test]
    fn test_keymap_context_matching() {
        let bindings = [
            KeyBinding::new("ctrl-a", ActionAlpha {}, None),
            KeyBinding::new("ctrl-a", ActionBeta {}, Some("pane")),
            KeyBinding::new("ctrl-a", ActionGamma {}, Some("editor && mode==full")),
        ];

        let keymap = test_keymap(bindings.clone());

        // global bindings are enabled in all contexts
        assert_eq!(keymap.binding_enabled(&bindings[0], &[]), Some(0));
        assert_eq!(
            keymap.binding_enabled(&bindings[0], &[KeyContext::parse("terminal").unwrap()]),
            Some(1)
        );

        // contextual bindings are enabled in contexts that match their predicate
        assert_eq!(
            keymap.binding_enabled(&bindings[1], &[KeyContext::parse("barf x=y").unwrap()]),
            None
        );
        assert_eq!(
            keymap.binding_enabled(&bindings[1], &[KeyContext::parse("pane x=y").unwrap()]),
            Some(1)
        );

        assert_eq!(
            keymap.binding_enabled(&bindings[2], &[KeyContext::parse("editor").unwrap()]),
            None
        );
        assert_eq!(
            keymap.binding_enabled(
                &bindings[2],
                &[KeyContext::parse("editor mode=full").unwrap()]
            ),
            Some(1)
        );
    }

    #[test]
    fn test_depth_precedence() {
        let bindings = [
            KeyBinding::new("ctrl-a", ActionBeta {}, Some("pane")),
            KeyBinding::new("ctrl-a", ActionGamma {}, Some("editor")),
        ];

        let keymap = test_keymap(bindings);

        let (result, pending) = keymap.bindings_for_input(
            &[Keystroke::parse("ctrl-a").unwrap()],
            &[
                KeyContext::parse("pane").unwrap(),
                KeyContext::parse("editor").unwrap(),
            ],
        );

        assert!(!pending);
        assert_eq!(result.len(), 2);
        assert!(result[0].action.partial_eq(&ActionGamma {}));
        assert!(result[1].action.partial_eq(&ActionBeta {}));
    }

    #[test]
    fn test_keymap_disabled() {
        let bindings = [
            KeyBinding::new("ctrl-a", ActionAlpha {}, Some("editor")),
            KeyBinding::new("ctrl-b", ActionAlpha {}, Some("editor")),
            KeyBinding::new("ctrl-a", NoAction {}, Some("editor && mode==full")),
            KeyBinding::new("ctrl-b", NoAction {}, None),
        ];

        let keymap = test_keymap(bindings);

        // binding is only enabled in a specific context
        assert!(
            keymap
                .bindings_for_input(
                    &[Keystroke::parse("ctrl-a").unwrap()],
                    &[KeyContext::parse("barf").unwrap()],
                )
                .0
                .is_empty()
        );
        assert!(
            !keymap
                .bindings_for_input(
                    &[Keystroke::parse("ctrl-a").unwrap()],
                    &[KeyContext::parse("editor").unwrap()],
                )
                .0
                .is_empty()
        );

        // binding is disabled in a more specific context
        assert!(
            keymap
                .bindings_for_input(
                    &[Keystroke::parse("ctrl-a").unwrap()],
                    &[KeyContext::parse("editor mode=full").unwrap()],
                )
                .0
                .is_empty()
        );

        // binding is globally disabled
        assert!(
            keymap
                .bindings_for_input(
                    &[Keystroke::parse("ctrl-b").unwrap()],
                    &[KeyContext::parse("barf").unwrap()],
                )
                .0
                .is_empty()
        );
    }

    #[test]
    /// Tests for https://github.com/zed-industries/zed/issues/30259
    fn test_multiple_keystroke_binding_disabled() {
        let bindings = [
            KeyBinding::new("space w w", ActionAlpha {}, Some("workspace")),
            KeyBinding::new("space w w", NoAction {}, Some("editor")),
        ];

        let keymap = test_keymap(bindings);

        let space = || Keystroke::parse("space").unwrap();
        let w = || Keystroke::parse("w").unwrap();

        let space_w = [space(), w()];
        let space_w_w = [space(), w(), w()];

        let workspace_context = || [KeyContext::parse("workspace").unwrap()];

        let editor_workspace_context = || {
            [
                KeyContext::parse("workspace").unwrap(),
                KeyContext::parse("editor").unwrap(),
            ]
        };

        // Ensure `space` results in pending input on the workspace, but not editor
        let space_workspace = keymap.bindings_for_input(&[space()], &workspace_context());
        assert!(space_workspace.0.is_empty());
        assert!(space_workspace.1);

        let space_editor = keymap.bindings_for_input(&[space()], &editor_workspace_context());
        assert!(space_editor.0.is_empty());
        assert!(!space_editor.1);

        // Ensure `space w` results in pending input on the workspace, but not editor
        let space_w_workspace = keymap.bindings_for_input(&space_w, &workspace_context());
        assert!(space_w_workspace.0.is_empty());
        assert!(space_w_workspace.1);

        let space_w_editor = keymap.bindings_for_input(&space_w, &editor_workspace_context());
        assert!(space_w_editor.0.is_empty());
        assert!(!space_w_editor.1);

        // Ensure `space w w` results in the binding in the workspace, but not in the editor
        let space_w_w_workspace = keymap.bindings_for_input(&space_w_w, &workspace_context());
        assert!(!space_w_w_workspace.0.is_empty());
        assert!(!space_w_w_workspace.1);

        let space_w_w_editor = keymap.bindings_for_input(&space_w_w, &editor_workspace_context());
        assert!(space_w_w_editor.0.is_empty());
        assert!(!space_w_w_editor.1);

        // Now test what happens if we have another binding defined AFTER the NoAction
        // that should result in pending
        let bindings = [
            KeyBinding::new("space w w", ActionAlpha {}, Some("workspace")),
            KeyBinding::new("space w w", NoAction {}, Some("editor")),
            KeyBinding::new("space w x", ActionAlpha {}, Some("editor")),
        ];
        let keymap = test_keymap(bindings);

        let space_editor = keymap.bindings_for_input(&[space()], &editor_workspace_context());
        assert!(space_editor.0.is_empty());
        assert!(space_editor.1);

        // Now test what happens if we have another binding defined BEFORE the NoAction
        // that should result in pending
        let bindings = [
            KeyBinding::new("space w w", ActionAlpha {}, Some("workspace")),
            KeyBinding::new("space w x", ActionAlpha {}, Some("editor")),
            KeyBinding::new("space w w", NoAction {}, Some("editor")),
        ];
        let keymap = test_keymap(bindings);

        let space_editor = keymap.bindings_for_input(&[space()], &editor_workspace_context());
        assert!(space_editor.0.is_empty());
        assert!(space_editor.1);

        // Now test what happens if we have another binding defined at a higher context
        // that should result in pending
        let bindings = [
            KeyBinding::new("space w w", ActionAlpha {}, Some("workspace")),
            KeyBinding::new("space w x", ActionAlpha {}, Some("workspace")),
            KeyBinding::new("space w w", NoAction {}, Some("editor")),
        ];
        let keymap = test_keymap(bindings);

        let space_editor = keymap.bindings_for_input(&[space()], &editor_workspace_context());
        assert!(space_editor.0.is_empty());
        assert!(space_editor.1);
    }

    #[test]
    fn test_override_multikey() {
        let bindings = [
            KeyBinding::new("ctrl-w left", ActionAlpha {}, Some("editor")),
            KeyBinding::new("ctrl-w", NoAction {}, Some("editor")),
        ];

        let keymap = test_keymap(bindings);

        // Ensure `space` results in pending input on the workspace, but not editor
        let (result, pending) = keymap.bindings_for_input(
            &[Keystroke::parse("ctrl-w").unwrap()],
            &[KeyContext::parse("editor").unwrap()],
        );
        assert!(result.is_empty());
        assert!(pending);

        let bindings = [
            KeyBinding::new("ctrl-w left", ActionAlpha {}, Some("editor")),
            KeyBinding::new("ctrl-w", ActionBeta {}, Some("editor")),
        ];

        let keymap = test_keymap(bindings);

        // Ensure `space` results in pending input on the workspace, but not editor
        let (result, pending) = keymap.bindings_for_input(
            &[Keystroke::parse("ctrl-w").unwrap()],
            &[KeyContext::parse("editor").unwrap()],
        );
        assert_eq!(result.len(), 1);
        assert!(!pending);
    }

    #[test]
    fn test_simple_disable() {
        let bindings = [
            KeyBinding::new("ctrl-x", ActionAlpha {}, Some("editor")),
            KeyBinding::new("ctrl-x", NoAction {}, Some("editor")),
        ];

        let keymap = test_keymap(bindings);

        // Ensure `space` results in pending input on the workspace, but not editor
        let (result, pending) = keymap.bindings_for_input(
            &[Keystroke::parse("ctrl-x").unwrap()],
            &[KeyContext::parse("editor").unwrap()],
        );
        assert!(result.is_empty());
        assert!(!pending);
    }

    #[test]
    fn test_disable_weaker_sources_only() {
        const USER: KeyBindingMetaIndex = KeyBindingMetaIndex(0);
        const VIM: KeyBindingMetaIndex = KeyBindingMetaIndex(1);
        const BASE: KeyBindingMetaIndex = KeyBindingMetaIndex(2);
        const DEFAULT: KeyBindingMetaIndex = KeyBindingMetaIndex(3);

        let editor_context = || [KeyContext::parse("editor").unwrap()];
        let ctrl_x = || [Keystroke::parse("ctrl-x").unwrap()];
        let add_bindings = |keymap: &mut Keymap, bindings: Vec<KeyBinding>| {
            keymap
                .set_layer(KeymapLayer::new(
                    InputLayerId::new("test.disable-weaker-sources"),
                    0,
                    bindings,
                ))
                .unwrap();
        };

        // A base keymap null disables a default binding in the same context.
        let mut keymap = Keymap::default();
        add_bindings(
            &mut keymap,
            vec![
                KeyBinding::new("ctrl-x", ActionAlpha {}, Some("editor")).with_meta(DEFAULT),
                KeyBinding::new("ctrl-x", NoAction {}, Some("editor")).with_meta(BASE),
            ],
        );
        let (result, _) = keymap.bindings_for_input(&ctrl_x(), &editor_context());
        assert!(result.is_empty());

        // A user binding is not affected by base keymap or default nulls.
        let mut keymap = Keymap::default();
        add_bindings(
            &mut keymap,
            vec![
                KeyBinding::new("ctrl-x", NoAction {}, Some("editor")).with_meta(DEFAULT),
                KeyBinding::new("ctrl-x", NoAction {}, Some("editor")).with_meta(BASE),
                KeyBinding::new("ctrl-x", ActionBeta {}, None).with_meta(USER),
            ],
        );
        let (result, _) = keymap.bindings_for_input(&ctrl_x(), &editor_context());
        assert_eq!(result.len(), 1);
        assert!(result[0].action.partial_eq(&ActionBeta {}));

        // A user binding at a shallower context is not disabled by a deeper
        // base keymap null.
        let mut keymap = Keymap::default();
        add_bindings(
            &mut keymap,
            vec![
                KeyBinding::new("ctrl-x", NoAction {}, Some("editor")).with_meta(BASE),
                KeyBinding::new("ctrl-x", ActionBeta {}, Some("workspace")).with_meta(USER),
            ],
        );
        let (result, _) = keymap.bindings_for_input(
            &ctrl_x(),
            &[
                KeyContext::parse("workspace").unwrap(),
                KeyContext::parse("editor").unwrap(),
            ],
        );
        assert_eq!(result.len(), 1);
        assert!(result[0].action.partial_eq(&ActionBeta {}));

        // A vim binding survives a base keymap null, and a user null disables
        // everything.
        let mut keymap = Keymap::default();
        add_bindings(
            &mut keymap,
            vec![
                KeyBinding::new("ctrl-x", ActionAlpha {}, Some("editor")).with_meta(DEFAULT),
                KeyBinding::new("ctrl-x", NoAction {}, Some("editor")).with_meta(BASE),
                KeyBinding::new("ctrl-x", ActionGamma {}, Some("editor")).with_meta(VIM),
            ],
        );
        let (result, _) = keymap.bindings_for_input(&ctrl_x(), &editor_context());
        assert_eq!(result.len(), 1);
        assert!(result[0].action.partial_eq(&ActionGamma {}));

        let mut keymap = Keymap::default();
        add_bindings(
            &mut keymap,
            vec![
                KeyBinding::new("ctrl-x", ActionAlpha {}, Some("editor")).with_meta(DEFAULT),
                KeyBinding::new("ctrl-x", ActionGamma {}, Some("editor")).with_meta(VIM),
                KeyBinding::new("ctrl-x", NoAction {}, Some("editor")).with_meta(USER),
            ],
        );
        let (result, _) = keymap.bindings_for_input(&ctrl_x(), &editor_context());
        assert!(result.is_empty());
    }

    #[test]
    fn test_fail_to_disable() {
        // disabled at the wrong level
        let bindings = [
            KeyBinding::new("ctrl-x", ActionAlpha {}, Some("editor")),
            KeyBinding::new("ctrl-x", NoAction {}, Some("workspace")),
        ];

        let keymap = test_keymap(bindings);

        // Ensure `space` results in pending input on the workspace, but not editor
        let (result, pending) = keymap.bindings_for_input(
            &[Keystroke::parse("ctrl-x").unwrap()],
            &[
                KeyContext::parse("workspace").unwrap(),
                KeyContext::parse("editor").unwrap(),
            ],
        );
        assert_eq!(result.len(), 1);
        assert!(!pending);
    }

    #[test]
    fn test_disable_deeper() {
        let bindings = [
            KeyBinding::new("ctrl-x", ActionAlpha {}, Some("workspace")),
            KeyBinding::new("ctrl-x", NoAction {}, Some("editor")),
        ];

        let keymap = test_keymap(bindings);

        // Ensure `space` results in pending input on the workspace, but not editor
        let (result, pending) = keymap.bindings_for_input(
            &[Keystroke::parse("ctrl-x").unwrap()],
            &[
                KeyContext::parse("workspace").unwrap(),
                KeyContext::parse("editor").unwrap(),
            ],
        );
        assert_eq!(result.len(), 0);
        assert!(!pending);
    }

    #[test]
    fn test_pending_match_enabled() {
        let bindings = [
            KeyBinding::new("ctrl-x", ActionBeta, Some("vim_mode == normal")),
            KeyBinding::new("ctrl-x 0", ActionAlpha, Some("Workspace")),
        ];
        let keymap = test_keymap(bindings);

        let matched = keymap.bindings_for_input(
            &[Keystroke::parse("ctrl-x")].map(Result::unwrap),
            &[
                KeyContext::parse("Workspace"),
                KeyContext::parse("Pane"),
                KeyContext::parse("Editor vim_mode=normal"),
            ]
            .map(Result::unwrap),
        );
        assert_eq!(matched.0.len(), 1);
        assert!(matched.0[0].action.partial_eq(&ActionBeta));
        assert!(matched.1);
    }

    #[test]
    fn test_pending_match_enabled_extended() {
        let bindings = [
            KeyBinding::new("ctrl-x", ActionBeta, Some("vim_mode == normal")),
            KeyBinding::new("ctrl-x 0", NoAction, Some("Workspace")),
        ];
        let keymap = test_keymap(bindings);

        let matched = keymap.bindings_for_input(
            &[Keystroke::parse("ctrl-x")].map(Result::unwrap),
            &[
                KeyContext::parse("Workspace"),
                KeyContext::parse("Pane"),
                KeyContext::parse("Editor vim_mode=normal"),
            ]
            .map(Result::unwrap),
        );
        assert_eq!(matched.0.len(), 1);
        assert!(matched.0[0].action.partial_eq(&ActionBeta));
        assert!(!matched.1);
        let bindings = [
            KeyBinding::new("ctrl-x", ActionBeta, Some("Workspace")),
            KeyBinding::new("ctrl-x 0", NoAction, Some("vim_mode == normal")),
        ];
        let keymap = test_keymap(bindings);

        let matched = keymap.bindings_for_input(
            &[Keystroke::parse("ctrl-x")].map(Result::unwrap),
            &[
                KeyContext::parse("Workspace"),
                KeyContext::parse("Pane"),
                KeyContext::parse("Editor vim_mode=normal"),
            ]
            .map(Result::unwrap),
        );
        assert_eq!(matched.0.len(), 1);
        assert!(matched.0[0].action.partial_eq(&ActionBeta));
        assert!(!matched.1);
    }

    #[test]
    fn test_overriding_prefix() {
        let bindings = [
            KeyBinding::new("ctrl-x 0", ActionAlpha, Some("Workspace")),
            KeyBinding::new("ctrl-x", ActionBeta, Some("vim_mode == normal")),
        ];
        let keymap = test_keymap(bindings);

        let matched = keymap.bindings_for_input(
            &[Keystroke::parse("ctrl-x")].map(Result::unwrap),
            &[
                KeyContext::parse("Workspace"),
                KeyContext::parse("Pane"),
                KeyContext::parse("Editor vim_mode=normal"),
            ]
            .map(Result::unwrap),
        );
        assert_eq!(matched.0.len(), 1);
        assert!(matched.0[0].action.partial_eq(&ActionBeta));
        assert!(!matched.1);
    }

    #[test]
    fn test_context_precedence_with_same_source() {
        // Test case: User has both Workspace and Editor bindings for the same key
        // Editor binding should take precedence over Workspace binding
        let bindings = [
            KeyBinding::new("cmd-r", ActionAlpha {}, Some("Workspace")),
            KeyBinding::new("cmd-r", ActionBeta {}, Some("Editor")),
        ];

        let keymap = test_keymap(bindings);

        // Test with context stack: [Workspace, Editor] (Editor is deeper)
        let (result, _) = keymap.bindings_for_input(
            &[Keystroke::parse("cmd-r").unwrap()],
            &[
                KeyContext::parse("Workspace").unwrap(),
                KeyContext::parse("Editor").unwrap(),
            ],
        );

        // Both bindings should be returned, but Editor binding should be first (highest precedence)
        assert_eq!(result.len(), 2);
        assert!(result[0].action.partial_eq(&ActionBeta {})); // Editor binding first
        assert!(result[1].action.partial_eq(&ActionAlpha {})); // Workspace binding second
    }

    #[test]
    fn test_bindings_for_action() {
        let bindings = [
            KeyBinding::new("ctrl-a", ActionAlpha {}, Some("pane")),
            KeyBinding::new("ctrl-b", ActionBeta {}, Some("editor && mode == full")),
            KeyBinding::new("ctrl-c", ActionGamma {}, Some("workspace")),
            KeyBinding::new("ctrl-a", NoAction {}, Some("pane && active")),
            KeyBinding::new("ctrl-b", NoAction {}, Some("editor")),
        ];

        let keymap = test_keymap(bindings);

        assert_bindings(&keymap, &ActionAlpha {}, &["ctrl-a"]);
        assert_bindings(&keymap, &ActionBeta {}, &[]);
        assert_bindings(&keymap, &ActionGamma {}, &["ctrl-c"]);

        #[track_caller]
        fn assert_bindings(keymap: &Keymap, action: &dyn Action, expected: &[&str]) {
            let actual = keymap
                .bindings_for_action(action)
                .map(|binding| binding.keystrokes[0].inner().unparse())
                .collect::<Vec<_>>();
            assert_eq!(actual, expected, "{:?}", action);
        }
    }

    #[test]
    fn test_targeted_unbind_ignores_target_context() {
        let bindings = [
            KeyBinding::new("tab", ActionAlpha {}, Some("Editor")),
            KeyBinding::new("tab", ActionBeta {}, Some("Editor && showing_completions")),
            KeyBinding::new(
                "tab",
                Unbind("test_only::ActionAlpha".into()),
                Some("Editor && edit_prediction"),
            ),
        ];

        let keymap = test_keymap(bindings);

        let (result, pending) = keymap.bindings_for_input(
            &[Keystroke::parse("tab").unwrap()],
            &[KeyContext::parse("Editor showing_completions edit_prediction").unwrap()],
        );

        assert!(!pending);
        assert_eq!(result.len(), 1);
        assert!(result[0].action.partial_eq(&ActionBeta {}));
    }

    #[test]
    fn test_bindings_for_action_keeps_binding_for_narrower_targeted_unbind() {
        let bindings = [
            KeyBinding::new("tab", ActionAlpha {}, Some("Editor")),
            KeyBinding::new(
                "tab",
                Unbind("test_only::ActionAlpha".into()),
                Some("Editor && edit_prediction"),
            ),
            KeyBinding::new("tab", ActionBeta {}, Some("Editor && showing_completions")),
        ];

        let keymap = test_keymap(bindings);

        assert_bindings(&keymap, &ActionAlpha {}, &["tab"]);
        assert_bindings(&keymap, &ActionBeta {}, &["tab"]);

        #[track_caller]
        fn assert_bindings(keymap: &Keymap, action: &dyn Action, expected: &[&str]) {
            let actual = keymap
                .bindings_for_action(action)
                .map(|binding| binding.keystrokes[0].inner().unparse())
                .collect::<Vec<_>>();
            assert_eq!(actual, expected, "{:?}", action);
        }
    }

    #[test]
    fn test_bindings_for_action_removes_binding_for_broader_targeted_unbind() {
        let bindings = [
            KeyBinding::new("tab", ActionAlpha {}, Some("Editor && edit_prediction")),
            KeyBinding::new(
                "tab",
                Unbind("test_only::ActionAlpha".into()),
                Some("Editor"),
            ),
        ];

        let keymap = test_keymap(bindings);

        assert!(keymap.bindings_for_action(&ActionAlpha {}).next().is_none());
    }

    #[test]
    fn test_source_precedence_sorting() {
        // KeybindSource precedence: User (0) > Vim (1) > Base (2) > Default (3)
        // Test that user keymaps take precedence over default keymaps at the same context depth
        let mut default_binding = KeyBinding::new("cmd-r", ActionAlpha {}, Some("Editor"));
        default_binding.set_meta(KeyBindingMetaIndex(3)); // Default source
        let mut user_binding = KeyBinding::new("cmd-r", ActionBeta {}, Some("Editor"));
        user_binding.set_meta(KeyBindingMetaIndex(0)); // User source
        let keymap = Keymap::from_layers([
            KeymapLayer::new(InputLayerId::new("default"), 0, [default_binding]),
            KeymapLayer::new(InputLayerId::new("user"), 10, [user_binding]),
        ])
        .unwrap();

        // Test with Editor context stack
        let (result, _) = keymap.bindings_for_input(
            &[Keystroke::parse("cmd-r").unwrap()],
            &[KeyContext::parse("Editor").unwrap()],
        );

        // User binding should take precedence over default binding
        assert_eq!(result.len(), 2);
        assert!(result[0].action.partial_eq(&ActionBeta {}));
        assert!(result[1].action.partial_eq(&ActionAlpha {}));
    }
}
