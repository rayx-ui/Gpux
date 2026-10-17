//! One table from AccessKit roles to ARIA role names, shared by the query vocabulary
//! ([`crate::semantic_query`]) and the web platform's DOM mirror.

use accesskit::Role;

/// The ARIA role an AccessKit role is exposed as in a browser, or `None` for the roles that have
/// none of their own (text, which is a node's content, and generic containers).
pub fn aria_role(role: Role) -> Option<&'static str> {
    Some(match role {
        Role::Unknown | Role::GenericContainer | Role::Label | Role::TextRun | Role::LineBreak => {
            return None;
        }
        Role::Button | Role::DefaultButton => "button",
        Role::CheckBox => "checkbox",
        Role::RadioButton => "radio",
        Role::RadioGroup => "radiogroup",
        Role::Switch => "switch",
        Role::Link => "link",
        Role::TextInput
        | Role::MultilineTextInput
        | Role::PasswordInput
        | Role::EmailInput
        | Role::PhoneNumberInput
        | Role::UrlInput
        | Role::DateInput
        | Role::DateTimeInput
        | Role::WeekInput
        | Role::MonthInput
        | Role::TimeInput => "textbox",
        Role::SearchInput => "searchbox",
        Role::NumberInput | Role::SpinButton => "spinbutton",
        Role::ComboBox | Role::EditableComboBox => "combobox",
        Role::Slider => "slider",
        Role::ScrollBar => "scrollbar",
        Role::ProgressIndicator => "progressbar",
        Role::Meter => "meter",
        Role::Image | Role::Canvas | Role::SvgRoot => "img",
        Role::Heading => "heading",
        Role::Paragraph => "paragraph",
        Role::List => "list",
        Role::ListItem => "listitem",
        Role::ListBox => "listbox",
        Role::ListBoxOption | Role::MenuListOption => "option",
        Role::Menu | Role::MenuListPopup => "menu",
        Role::MenuBar => "menubar",
        Role::MenuItem => "menuitem",
        Role::MenuItemCheckBox => "menuitemcheckbox",
        Role::MenuItemRadio => "menuitemradio",
        Role::Tree => "tree",
        Role::TreeItem => "treeitem",
        Role::TreeGrid => "treegrid",
        Role::Table | Role::ListGrid => "table",
        Role::Grid => "grid",
        Role::Row => "row",
        Role::RowGroup => "rowgroup",
        Role::Cell => "cell",
        Role::GridCell => "gridcell",
        Role::ColumnHeader => "columnheader",
        Role::RowHeader => "rowheader",
        Role::Tab => "tab",
        Role::TabList => "tablist",
        Role::TabPanel => "tabpanel",
        Role::Toolbar => "toolbar",
        Role::Tooltip => "tooltip",
        Role::Dialog => "dialog",
        Role::AlertDialog => "alertdialog",
        Role::Alert => "alert",
        Role::Status => "status",
        Role::Log => "log",
        Role::Marquee => "marquee",
        Role::Timer => "timer",
        Role::Group | Role::Details | Role::Pane => "group",
        Role::Region | Role::Section => "region",
        Role::Form => "form",
        Role::Search => "search",
        Role::Navigation => "navigation",
        Role::Main => "main",
        Role::Banner | Role::Header => "banner",
        Role::ContentInfo | Role::Footer => "contentinfo",
        Role::Complementary => "complementary",
        Role::Article => "article",
        Role::Document | Role::RootWebArea => "document",
        Role::Application => "application",
        Role::Feed => "feed",
        Role::Figure => "figure",
        Role::Note => "note",
        Role::Term => "term",
        Role::Definition => "definition",
        Role::Blockquote => "blockquote",
        Role::Code => "code",
        Role::Emphasis => "emphasis",
        Role::Strong => "strong",
        Role::Math => "math",
        Role::Splitter => "separator",
        Role::Window => "window",
        _ => return None,
    })
}

/// The name a [`crate::semantic_query::Query`] matches a role by: its ARIA role, `text` for a
/// `Label` (text), `generic` for a generic container, and for a role ARIA has no name for, the
/// AccessKit role's name in lower case.
pub fn query_role_name(role: Role) -> String {
    match role {
        Role::Label => "text".into(),
        Role::GenericContainer => "generic".into(),
        role => aria_role(role)
            .map(str::to_string)
            .unwrap_or_else(|| format!("{role:?}").to_ascii_lowercase()),
    }
}
