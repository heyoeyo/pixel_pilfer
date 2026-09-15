use image::Rgba;

use crate::imports::types::Colormap;

// --------------------------------------------------------------------------------------------------------------------

pub fn make_cmap_inferno() -> Colormap {
    /*
    Make colormap approximating the 'inferno' colormap from opencv. See:
    https://docs.opencv.org/4.13.0/d3/d50/group__imgproc__colormap.html
    */
    let cmap = vec![
        [250, 255, 165],
        [250, 205, 50],
        [250, 130, 15],
        [230, 95, 45],
        [200, 60, 80],
        [125, 30, 110],
        [105, 25, 110],
        [30, 10, 65],
        [20, 10, 30],
    ];
    return make_colormap_lut(&cmap);
}

pub fn make_colormap_lut(rgb_color_sequence: &Vec<[u8; 3]>) -> Colormap {
    /*  Helper used to make a 1024-entry lookup-table holding colors, according to the provide list of colors */

    // Sanity check
    let num_colors = rgb_color_sequence.len();
    assert!(num_colors > 0, "Bad colormap! Must have at least 1 color");

    // Build colormap look-up table
    let max_color_idx = (num_colors - 1) as f32;
    let mut lut: Colormap = [Rgba([0, 0, 0, 255]); 1024];
    for (idx, color) in lut.iter_mut().enumerate() {
        let idx_norm = max_color_idx * idx as f32 / 1023.0;
        let idx_a = idx_norm.floor();
        let idx_b = idx_norm.ceil().min(max_color_idx);
        let lerp_t = idx_norm - idx_a;

        let lerp_color = lerp_rgb(
            &rgb_color_sequence[idx_a as usize],
            &rgb_color_sequence[idx_b as usize],
            lerp_t,
        );
        color[0] = lerp_color[0];
        color[1] = lerp_color[1];
        color[2] = lerp_color[2];
    }

    return lut;
}

pub fn lerp_rgb(color_a: &[u8; 3], color_b: &[u8; 3], mix: f32) -> [u8; 3] {
    /* Function used to blend from color_a (mix=0.0) to color_b (mix=1.0) */
    let inv_mix = 1.0 - mix;
    let mut new_color: [u8; 3] = [0, 0, 0];
    for idx in 0..3 {
        new_color[idx] = ((color_a[idx] as f32 * inv_mix) + (color_b[idx] as f32) * mix).round() as u8;
    }
    return new_color;
}
