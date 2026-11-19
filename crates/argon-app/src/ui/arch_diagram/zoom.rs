pub const ZOOM_STEPS: [f32; 5] = [0.5, 0.75, 1., 1.25, 1.5];
pub const DEFAULT_STEP: usize = 2;
pub const EXCERPT_MIN_ZOOM: f32 = 0.6;

pub fn step_zoom(step: usize) -> f32 {
    ZOOM_STEPS[step.min(ZOOM_STEPS.len() - 1)]
}

pub fn zoom_in(step: usize) -> usize {
    (step + 1).min(ZOOM_STEPS.len() - 1)
}

pub fn zoom_out(step: usize) -> usize {
    step.min(ZOOM_STEPS.len() - 1).saturating_sub(1)
}

pub fn can_zoom_in(step: usize) -> bool {
    step < ZOOM_STEPS.len() - 1
}

pub fn can_zoom_out(step: usize) -> bool {
    step > 0
}

pub fn zoom_percent(step: usize) -> String {
    format!("{}%", (step_zoom(step) * 100.).round() as u32)
}

pub fn fit_step(base_width: f32, visible_width: f32) -> usize {
    if !(visible_width.is_finite() && visible_width > 0. && base_width.is_finite())
        || base_width <= 0.
    {
        return DEFAULT_STEP;
    }
    ZOOM_STEPS
        .iter()
        .rposition(|zoom| base_width * zoom <= visible_width)
        .unwrap_or(0)
}

pub fn excerpt_zoom(base_width: f32, available_width: Option<f32>) -> f32 {
    match available_width {
        Some(available) if available > 0. && base_width > 0. => {
            (available / base_width).clamp(EXCERPT_MIN_ZOOM, 1.)
        }
        _ => 1.,
    }
}
