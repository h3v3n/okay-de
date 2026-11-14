use smithay::{
    backend::renderer::{
        damage::{OutputDamageTracker, RenderOutputResult},
        element::{
            surface::{render_elements_from_surface_tree, WaylandSurfaceRenderElement},
            solid::{SolidColorBuffer, SolidColorRenderElement},
            AsRenderElements, Kind,
        },
        gles::{GlesError, GlesRenderer, GlesTarget},
        Color32F, ImportAll, Renderer,
    },
    desktop::{
        layer_map_for_output, utils::send_frames_surface_tree,
    },
    output::Output,
    reexports::wayland_server::Resource,
    render_elements,
    utils::{Logical, Physical, Point, Rectangle, Scale},
    wayland::shell::wlr_layer::Layer,
};

use crate::state::OkayState;

render_elements! {
    pub OkayRenderElements<R> where R: Renderer + ImportAll;
    Surface=WaylandSurfaceRenderElement<R>,
    Solid=SolidColorRenderElement,
}

/// Collects all render elements for a given `Output` in front-to-back order
/// (Overlay -> Top Layer -> Focused/Visible Windows & Borders -> Bottom/Background Layer).
///
/// Designed to be easily extended with custom GLSL post-processing shaders,
/// window animations, and native compositor UI widgets.
pub fn collect_output_elements(
    state: &OkayState,
    renderer: &mut GlesRenderer,
    output: &Output,
    scale: Scale<f64>,
) -> Vec<OkayRenderElements<GlesRenderer>> {
    let mut elements: Vec<OkayRenderElements<GlesRenderer>> = Vec::new();
    let Some(output_geo) = state.space.output_geometry(output) else {
        return elements;
    };

    let layer_map = layer_map_for_output(output);

    // 1. Overlay and Top layer-shell surfaces (notifications, bars, docks, launchers)
    for layer_type in [Layer::Overlay, Layer::Top] {
        for layer in layer_map.layers_on(layer_type).rev() {
            if let Some(geo) = layer_map.layer_geometry(layer) {
                let loc = geo.loc.to_physical_precise_round(scale);
                let surface_elems: Vec<OkayRenderElements<GlesRenderer>> =
                    render_elements_from_surface_tree(
                        renderer,
                        layer.wl_surface(),
                        loc,
                        scale,
                        1.0,
                        Kind::Unspecified,
                    );
                elements.extend(surface_elems);
            }
        }
    }

    // 2. Visible XDG windows on the active workspace (top-to-bottom)
    let focused_id = state
        .seat
        .get_keyboard()
        .and_then(|k| k.current_focus())
        .map(|s| s.id());

    for window in state.space.elements().rev() {
        if !state.workspaces.is_visible(window) {
            continue;
        }
        let Some(win_loc) = state.space.element_location(window) else {
            continue;
        };

        // Translate global space coordinates into output-local coordinates
        let local_loc = win_loc - output_geo.loc;
        let phys_loc: Point<i32, Physical> = local_loc.to_physical_precise_round(scale);

        // Window surfaces (toplevel + subsurfaces + popups)
        let win_elements: Vec<OkayRenderElements<GlesRenderer>> =
            window.render_elements(renderer, phys_loc, scale, 1.0);
        elements.extend(win_elements);

        // Server-side border decoration around the window geometry
        let is_focused = window
            .toplevel()
            .map(|t| Some(t.wl_surface().id()) == focused_id)
            .unwrap_or(false);

        let border_color = if is_focused {
            state.theme.active_border_color
        } else {
            state.theme.inactive_border_color
        };

        let win_geo = window.geometry();
        let border_geo = Rectangle::new(
            local_loc + win_geo.loc - Point::from((state.theme.border_width, state.theme.border_width)),
            win_geo.size
                + (
                    state.theme.border_width * 2,
                    state.theme.border_width * 2,
                )
                    .into(),
        );

        let border_elems = build_border_elements(
            border_geo,
            state.theme.border_width,
            Color32F::from(border_color),
            scale,
        );
        elements.extend(border_elems);
    }

    // 3. Bottom and Background layer-shell surfaces (wallpapers, desktop widgets)
    for layer_type in [Layer::Bottom, Layer::Background] {
        for layer in layer_map.layers_on(layer_type).rev() {
            if let Some(geo) = layer_map.layer_geometry(layer) {
                let loc = geo.loc.to_physical_precise_round(scale);
                let surface_elems: Vec<OkayRenderElements<GlesRenderer>> =
                    render_elements_from_surface_tree(
                        renderer,
                        layer.wl_surface(),
                        loc,
                        scale,
                        1.0,
                        Kind::Unspecified,
                    );
                elements.extend(surface_elems);
            }
        }
    }

    elements
}

/// Builds 4 crisp solid-color border strips (top, bottom, left, right) around `rect`.
fn build_border_elements(
    rect: Rectangle<i32, Logical>,
    thickness: i32,
    color: Color32F,
    scale: Scale<f64>,
) -> Vec<OkayRenderElements<GlesRenderer>> {
    if thickness <= 0 || rect.size.w <= 0 || rect.size.h <= 0 {
        return Vec::new();
    }

    let strips = [
        // Top
        Rectangle::new(rect.loc, (rect.size.w, thickness).into()),
        // Bottom
        Rectangle::new(
            (rect.loc.x, rect.loc.y + rect.size.h - thickness).into(),
            (rect.size.w, thickness).into(),
        ),
        // Left
        Rectangle::new(
            (rect.loc.x, rect.loc.y + thickness).into(),
            (thickness, (rect.size.h - 2 * thickness).max(0)).into(),
        ),
        // Right
        Rectangle::new(
            (rect.loc.x + rect.size.w - thickness, rect.loc.y + thickness).into(),
            (thickness, (rect.size.h - 2 * thickness).max(0)).into(),
        ),
    ];

    strips
        .into_iter()
        .filter(|s| s.size.w > 0 && s.size.h > 0)
        .map(|strip| {
            let buf = SolidColorBuffer::new(strip.size, color);
            let phys_loc = strip.loc.to_physical_precise_round(scale);
            OkayRenderElements::Solid(SolidColorRenderElement::from_buffer(
                &buf,
                phys_loc,
                scale,
                1.0,
                Kind::Unspecified,
            ))
        })
        .collect()
}

/// Renders a single frame for `output` using `OutputDamageTracker`.
pub fn render_output<'a, 'd>(
    state: &OkayState,
    renderer: &mut GlesRenderer,
    framebuffer: &mut GlesTarget<'a>,
    damage_tracker: &'d mut OutputDamageTracker,
    output: &Output,
    age: usize,
) -> Result<RenderOutputResult<'d>, smithay::backend::renderer::damage::Error<GlesError>> {
    let scale = Scale::from(output.current_scale().fractional_scale());
    let elements = collect_output_elements(state, renderer, output, scale);
    let clear_color = Color32F::from(state.theme.background_color);

    damage_tracker.render_output(renderer, framebuffer, age, &elements, clear_color)
}

/// Sends Wayland frame callbacks to all mapped windows and layer-shell surfaces on `output`.
pub fn send_frame_callbacks(state: &OkayState, output: &Output) {
    let time = state.start_time.elapsed();

    for window in state.space.elements() {
        if state.workspaces.is_visible(window) {
            window.send_frame(output, time, Some(std::time::Duration::ZERO), |_, _| {
                Some(output.clone())
            });
        }
    }

    let layer_map = layer_map_for_output(output);
    for layer in layer_map.layers() {
        send_frames_surface_tree(
            layer.wl_surface(),
            output,
            time,
            Some(std::time::Duration::ZERO),
            |_, _| Some(output.clone()),
        );
    }
}
