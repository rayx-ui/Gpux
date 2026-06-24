#[test]
fn test_derive_context() {
    use gpui_macros::{AppContext, VisualContext};
    use gpux::{App, Window};

    #[derive(AppContext, VisualContext)]
    struct _MyCustomContext<'a, 'b> {
        #[app]
        app: &'a mut App,
        #[window]
        window: &'b mut Window,
    }
}
