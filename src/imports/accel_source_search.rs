use crate::imports;
use imports::state2d::{index_from_xy, xy_from_index};
use rand::random_range;
use rand::seq::SliceRandom;

// --------------------------------------------------------------------------------------------------------------------

pub struct SourceSearch {
    /*
    This is an acceleration structure used to help sample 'nearby unvisited' points around a given
    position faster than if we just directly iterate over all nearby points to find an unvisited one.

    It relies on the fact that all possible 'nearest unvisited' points are always neighbors of the
    visited points. They therefore form a boundary/wavefront around the sampled points.
    Typically there will be too many unvisited points to just directly iterate through to find the
    'closest one', so this structure holds the points in a (sparse) grid. To find a nearby point,
    we first check nearby grid cells until we find one with points, then iterate over only the points
    in the cell to find a 'closest' point. Note that the grid is stored in a 1D layout!
    */
    src_wh: (usize, usize),
    cell_side_length: usize,
    grid_wh: (usize, usize),
    uv_pts_grid: Vec<Vec<(usize, usize)>>,
    nearest_search_patterns: Vec<Vec<(i32, i32)>>,
}

impl SourceSearch {
    pub fn new(grid_cell_size: usize) -> Self {
        return Self {
            src_wh: (0, 0),
            cell_side_length: grid_cell_size,
            grid_wh: (0, 0),
            uv_pts_grid: Vec::new(),
            nearest_search_patterns: Vec::new(),
        };
    }

    pub fn clear(&mut self) {
        // Clear all cells, don't clear the grid structure itself
        for cell_idx in 0..self.uv_pts_grid.len() {
            self.uv_pts_grid[cell_idx].clear();
        }
    }

    pub fn resize(&mut self, source_wh: (usize, usize)) {
        // Figure out how many grid cells there should be
        let grid_h = (source_wh.1 as f32 / self.cell_side_length as f32).ceil() as usize;
        let grid_w = (source_wh.0 as f32 / self.cell_side_length as f32).ceil() as usize;

        // Resize existing vector to store required grid data
        let num_grid_cells = grid_w * grid_h;
        let num_extra_cells = num_grid_cells.saturating_sub(self.uv_pts_grid.capacity());
        self.uv_pts_grid.reserve(num_extra_cells);

        // Resize cells to fit points as needed
        let pts_per_grid_cell = self.cell_side_length * self.cell_side_length;
        let target_cell_capacity = pts_per_grid_cell / 2;
        for grid_y in 0..grid_h {
            for grid_x in 0..grid_w {
                let grid_idx = index_from_xy(grid_x, grid_y, grid_w);
                if grid_idx < self.uv_pts_grid.len() {
                    let curr_cell_capaity = self.uv_pts_grid[grid_idx].capacity();
                    let num_extra_ptrs_per_cell = target_cell_capacity.saturating_sub(curr_cell_capaity);
                    self.uv_pts_grid[grid_idx].reserve(num_extra_ptrs_per_cell);
                } else {
                    self.uv_pts_grid.push(Vec::with_capacity(target_cell_capacity));
                }
            }
        }

        // Update stored state
        self.grid_wh = (grid_w, grid_h);
        self.src_wh = source_wh;
        self.nearest_search_patterns = self.make_search_patterns();
    }

    pub fn get_nearest_point(&mut self, source_points: &Vec<usize>) -> Option<usize> {
        /*
            Function which tries to find an unvisited source point that is closest to one of the given points.
            This check is somewhat complex, as it attempts to use caching tricks to accelerate things.

            Possible 'nearest' points are held in a (sparse) grid. This function checks the nearest
            grid cell(s) to see if they contain any points. Once we find a non-empty cell,
            we iterate over all held points to see which is closest to our given points.

            This implementation is still slow as the image/grid size increases, as this can lead
            to 1000's of buckets to check when the nearest sample is far away. This could be sped
            up with a quadtree-like structure in the future if needed...
        */

        // Initialize 'best' checks for re-use
        let mut best_grid_and_cell_idxs: Vec<(usize, usize)> = Vec::with_capacity(128);
        let mut best_nearest_dist = usize::MAX;

        // Pre-compute the grid cell location of the given source points
        let src_pxy_gxy: Vec<((usize, usize), (i32, i32))> = source_points
            .iter()
            .map(|pxidx| {
                let src_px_xy = xy_from_index(*pxidx, self.src_wh.0);
                let src_grid_x = (src_px_xy.0 / self.cell_side_length) as i32;
                let src_grid_y = (src_px_xy.1 / self.cell_side_length) as i32;
                let src_grid_xy = (src_grid_x, src_grid_y);
                (src_px_xy, src_grid_xy)
            })
            .collect();

        // Loop over pre-defined sets of search patterns (in the form of relative offsets from current position)
        let mut first_src_check_idx = 0;
        let num_src_pts = source_points.len();
        for dxdys_per_radius in &self.nearest_search_patterns {
            // Repeat current radius search for every input point
            // -> Change first checked point each time to reduce bias
            first_src_check_idx = (first_src_check_idx + 1) % num_src_pts;
            for check_idx_offset in 0..num_src_pts {
                let (src_px_xy, src_grid_xy) = src_pxy_gxy[(first_src_check_idx + check_idx_offset) % num_src_pts];

                // Search nearby (based on dx, dy offsets) grid cells for points
                best_grid_and_cell_idxs.clear();
                for (dx, dy) in dxdys_per_radius {
                    // Skip cells that land outside of the grid
                    let offset_grid_x: i32 = src_grid_xy.0 + dx;
                    let offset_grid_y: i32 = src_grid_xy.1 + dy;
                    if offset_grid_x < 0 || offset_grid_x >= self.grid_wh.0 as i32 {
                        continue;
                    } else if offset_grid_y < 0 || offset_grid_y >= self.grid_wh.1 as i32 {
                        continue;
                    }

                    // If the grid cell contains points, check if any are the new 'closest' point
                    // -> Note, there can be multiple points equally close
                    // -> We need to consider all of them to avoid biased sampling, so we store a list
                    let grid_idx = index_from_xy(offset_grid_x as usize, offset_grid_y as usize, self.grid_wh.0);
                    let active_cell = &self.uv_pts_grid[grid_idx];
                    if active_cell.len() > 0 {
                        let (nearest_cell_idx, nearest_dist) = self.search_grid_cell(active_cell, src_px_xy);
                        if nearest_dist <= best_nearest_dist {
                            if nearest_dist < best_nearest_dist {
                                best_grid_and_cell_idxs.clear();
                            }
                            best_nearest_dist = nearest_dist;
                            best_grid_and_cell_idxs.push((grid_idx, nearest_cell_idx));
                        }
                    }
                }

                // If we got 1 or more 'closest' points, take one randomly as final output
                let num_best = best_grid_and_cell_idxs.len();
                if num_best > 0 {
                    let (best_gidx, best_cidx) = best_grid_and_cell_idxs[random_range(0..num_best)];
                    let nearest_xy = self.uv_pts_grid[best_gidx].swap_remove(best_cidx);
                    let nearest_pxidx = index_from_xy(nearest_xy.0, nearest_xy.1, self.src_wh.0);
                    return Some(nearest_pxidx);
                }
            }
        }

        // This shouldn't happen normally, if we get here, it means we checked all grid cells and
        // didn't find a single point! Probably means something went wrong with point recording
        // -> Will also occur if there aren't any source points left (e.g. if 'undersizing' the mapping)
        return None;
    }

    fn search_grid_cell(&self, cell: &Vec<(usize, usize)>, start_xy: (usize, usize)) -> (usize, usize) {
        /*
         Iterates over all stored points (within the given cell), and computes the
         corresponding (manhattan) distance to find the closest point.
         Assumes the provided cell contains at least 1 point!
         Returns:
             (index_of_closest_entry, closest_distance)
        */

        // Loop over all cell entries and find xy closest to given point (manhattan distance)
        let mut closest_dist = usize::MAX;
        let mut closest_idx = 0;
        let mut item_idx = 0;
        for (item_x, item_y) in cell {
            let item_dist = start_xy.0.abs_diff(*item_x) + start_xy.1.abs_diff(*item_y);
            if item_dist < closest_dist {
                closest_dist = item_dist;
                closest_idx = item_idx;
            }
            item_idx += 1;
        }
        return (closest_idx, closest_dist);
    }

    pub fn add_search_points(&mut self, search_points: &Vec<usize>) {
        /*
        Helper used to record points to search in the future.
        Handles the assignment of each point to the appropriate grid cell
        for caching/quick lookup
        */

        // Add new points to grid
        for pt in search_points {
            let src_px_xy = xy_from_index(*pt, self.src_wh.0);
            let grid_x = src_px_xy.0 / self.cell_side_length;
            let grid_y = src_px_xy.1 / self.cell_side_length;
            let grid_idx = index_from_xy(grid_x, grid_y, self.grid_wh.0);
            self.uv_pts_grid[grid_idx].push(src_px_xy);
        }
    }

    fn make_search_patterns(&self) -> Vec<Vec<(i32, i32)>> {
        /*
        This function generates a list-of-lists of (dx,dy) values, where
        each (dx,dy) is meant as a grid offset (from some central cell).
        Together, these form a 'search pattern' that grows outwardly from
        the (assumed) central cell location. This is used to quickly figure
        out where the 'next closest' grid cells should be, for sampling.

        The pattern in this case is based on manhattan distance, where each
        new list of (dx,dy) values is one step further away
        */

        // Figure out how many patterns we need to compute/store
        let max_num_steps = 2 * self.grid_wh.0.max(self.grid_wh.1) - 1;
        let mut search_patterns: Vec<Vec<(i32, i32)>> = Vec::with_capacity(max_num_steps);

        // Record (special) radius 0/1 case together
        // -> This helps avoid always searching exclusively inside the current cell
        let mut r1_pattern = vec![(0, 0), (1, 0), (-1, 0), (0, 1), (0, -1)];
        r1_pattern.shuffle(&mut rand::rng());
        search_patterns.push(r1_pattern);

        // Now we step away some amount (radius) and generate the 4 diagonal lines worth of new grid cells
        for radius in 2..(max_num_steps as i32) {
            let mut xy_offsets = Vec::with_capacity(radius as usize);
            for step in 0..radius {
                let (nr, pr) = (-radius + step, radius - step);
                let (nz, pz) = (0 - step, 0 + step);
                xy_offsets.push((nr, nz));
                xy_offsets.push((nr, pz));
                xy_offsets.push((pr, nz));
                xy_offsets.push((pr, pz));
            }
            xy_offsets.shuffle(&mut rand::rng());
            search_patterns.push(xy_offsets);
        }

        return search_patterns;
    }
}
