mod anchored;
mod animation;
mod canvas;
mod deferred;
mod div;
mod html_element;
mod html_island;
mod image_cache;
mod img;
mod list;
mod surface;
mod svg;
mod text;
mod uniform_list;

pub use anchored::*;
pub use animation::*;
pub use canvas::*;
pub use deferred::*;
pub use div::*;
pub use html_element::*;
pub use html_island::*;
pub use image_cache::*;
pub use img::*;
pub use list::*;
pub use surface::*;
pub use svg::*;
pub use text::*;
pub use uniform_list::*;

#[cfg(test)]
mod tests {
    use crate::{
        HtmlElement, HtmlElementContentFit, HtmlElementInputMode, HtmlElementWheelBehavior,
        HtmlIsland, HtmlIslandInputMode, IntoElement as _, ParentElement as _, div, html_element,
        html_island,
    };

    #[test]
    fn html_island_public_api_accepts_trusted_html_input_mode_and_fallback() {
        let island: HtmlIsland = html_island("settings-form")
            .html("<button>Save</button>")
            .input_mode(HtmlIslandInputMode::DomOwned)
            .fallback(div().child("unsupported"));

        island.into_element();
    }

    #[test]
    fn html_element_public_api_accepts_trusted_html_input_mode_keep_alive_and_fallback() {
        let element: HtmlElement = html_element("provider-frame")
            .trusted_html("<iframe title=\"Provider\" srcdoc=\"ready\"></iframe>")
            .input_mode(HtmlElementInputMode::DomOwned)
            .content_fit(HtmlElementContentFit::Fill)
            .wheel_behavior(HtmlElementWheelBehavior::CaptureForward)
            .extra_styles("border: 1px solid #fff; border-radius: 8px;")
            .keep_alive(true)
            .fallback(div().child("provider unavailable"));

        element.into_element();
    }
}
