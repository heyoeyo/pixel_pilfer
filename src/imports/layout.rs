// No imports

// --------------------------------------------------------------------------------------------------------------------

pub struct DisplayRegion {
    /* Used to indicate placement/size of display data (for use with image-overlay/blitting) */
    pub xy: (i32, i32),
    pub wh: (u32, u32),
}

#[derive(PartialEq)]
pub enum DisplayLayout {
    HStack,
    NoText,
    Solo,
}

// --------------------------------------------------------------------------------------------------------------------

pub fn get_hstack_layout(
    display_wh: (u32, u32),
    left_wh: (u32, u32),
    right_wh: (u32, u32),
    gap_size_px: u32,
    anchor: Option<(f32, f32)>,
) -> (DisplayRegion, [DisplayRegion; 2]) {
    /*
    Helper used to figure out the placement & sizing of two images that are meant
    to be stacked side-by-side and displayed in the provided display area. Optionally
    takes an anchor that decides alignment/padding if the images don't perfectly fit.
    Returns:
        outer_bounding_region, [left_region, right_region]
    */

    // For convenience
    let (w_disp, h_disp) = display_wh;
    let (w_left_in, h_left_in) = (left_wh.0 as f32, left_wh.1 as f32);
    let (w_right_in, h_right_in) = (right_wh.0 as f32, right_wh.1 as f32);
    let w_disp_available = (w_disp - gap_size_px) as f32;

    // Find combined width when both images are scaled to fit display height
    let h_combined = h_disp as f32;
    let w_left_scaled: f32 = h_combined * w_left_in / h_left_in;
    let w_right_scaled: f32 = h_combined * w_right_in / h_right_in;
    let w_combined: f32 = w_left_scaled + w_right_scaled;

    // Scale both images to fit to a smaller available width if needed
    let disp_scale_factor = (w_disp_available / w_combined).min(1.0);
    let w_left_out = (w_left_scaled * disp_scale_factor).round() as u32;
    let w_right_out = (w_right_scaled * disp_scale_factor).round() as u32;
    let h_combined_out = (h_combined * disp_scale_factor).round() as u32;

    // Figure out padding if the combined images don't fit perfectly into the display area
    let (x_anchor, y_anchor) = anchor.unwrap_or((0.5, 0.5));
    let pad_total_w = w_disp.saturating_sub(w_left_out + gap_size_px + w_right_out);
    let pad_total_h = h_disp.saturating_sub(h_combined_out);
    let pad_left = (pad_total_w as f32 * x_anchor).round() as i32;
    let pad_top = (pad_total_h as f32 * y_anchor).round() as i32;

    let left_layout = DisplayRegion {
        xy: (pad_left, pad_top),
        wh: (w_left_out, h_combined_out),
    };
    let right_layout = DisplayRegion {
        xy: (pad_left + w_left_out as i32 + gap_size_px as i32, pad_top as i32),
        wh: (w_right_out, h_combined_out),
    };
    let outer_layout = DisplayRegion {
        xy: left_layout.xy,
        wh: (left_layout.wh.0 + gap_size_px + right_layout.wh.0, left_layout.wh.1),
    };

    return (outer_layout, [left_layout, right_layout]);
}

pub fn get_solo_layout(display_wh: (u32, u32), solo_wh: (u32, u32), anchor: Option<(f32, f32)>) -> DisplayRegion {
    /*
    Helper used to figure out the placement & sizing of a solo image that is meant to
    be scaled to fit into a display region. Optionally takes an anchor, which will
    alter the padding/alignment when the image doesn't fully fit (defaults to centering).
    */

    // For convenience
    let (w_disp, h_disp) = (display_wh.0 as f32, display_wh.1 as f32);
    let (w_solo, h_solo) = (solo_wh.0 as f32, solo_wh.1 as f32);

    // Find scaling factor that fits solo image into display
    let w_scale_factor: f32 = w_disp / w_solo;
    let h_scale_factor: f32 = h_disp / h_solo;
    let disp_scale_factor = w_scale_factor.min(h_scale_factor);
    let w_out = (w_solo * disp_scale_factor).round() as u32;
    let h_out = (h_solo * disp_scale_factor).round() as u32;

    // Figure out padding if the image doesn't fit perfectly into the display area
    let (x_anchor, y_anchor) = anchor.unwrap_or((0.5, 0.5));
    let pad_total_w = display_wh.0.saturating_sub(w_out);
    let pad_total_h = display_wh.1.saturating_sub(h_out);
    let pad_left = (pad_total_w as f32 * x_anchor).round() as i32;
    let pad_top = (pad_total_h as f32 * y_anchor).round() as i32;

    return DisplayRegion {
        xy: (pad_left, pad_top),
        wh: (w_out, h_out),
    };
}
