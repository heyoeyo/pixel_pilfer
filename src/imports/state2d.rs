use rand::random_range;

// --------------------------------------------------------------------------------------------------------------------

// Helpers types used to better indicate the use of pixel-indexing as inputs/outputs from functions
#[allow(non_camel_case_types)]
type pidx = usize;
#[allow(non_camel_case_types)]
type pidx_u32 = u32;

// --------------------------------------------------------------------------------------------------------------------

pub struct State2D<T> {
    /*
    This struct represents a 2D grid of values, but uses a 1D vector for efficiency.
    Methods are provided to make it easier to work with, as though it were 2D
    */
    state: Vec<T>,
    pub width: usize,
    pub height: usize,
    _init_state: T,
}

impl<T> State2D<T>
where
    T: Copy,
{
    pub fn new(initial_state: T, width: usize, height: usize) -> Self {
        Self {
            state: vec![initial_state; width * height],
            width: width,
            height: height,
            _init_state: initial_state.clone(),
        }
    }

    pub fn resize(&mut self, new_width: usize, new_height: usize) {
        self.width = new_width;
        self.height = new_height;
        self.state.resize(new_width * new_height, self._init_state);
    }

    pub fn fill(&mut self, new_state: T) {
        self.state.fill(new_state);
    }

    pub fn read(&self, point_index: pidx) -> T {
        self.state[point_index]
    }

    pub fn set_state(&mut self, new_state: T, point_index: pidx) {
        self.state[point_index] = new_state;
    }

    pub fn dimensions(&self) -> (usize, usize) {
        return (self.width, self.height);
    }

    #[allow(unused)]
    pub fn iter_xy(&self) -> impl Iterator<Item = (usize, usize, &T)> {
        /*  Meant to use in a for loop like: for (x_idx, y_idx, state_value) { ... } */
        self.state.iter().enumerate().map(|(idx, item)| {
            let (x, y) = xy_from_index(idx, self.width);
            (x, y, item)
        })
    }

    #[allow(unused)]
    pub fn iter_1d(&self) -> impl Iterator<Item = &T> {
        return self.state.iter();
    }
}

// . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . .

#[derive(PartialEq, Clone, Copy)]
enum VisitState {
    Unvisited,
    Searched,
    Visited,
}

pub struct Visited2D {
    visited: State2D<VisitState>,
}

impl Visited2D {
    pub fn new(size_wh: (usize, usize)) -> Self {
        Self {
            visited: State2D::new(VisitState::Unvisited, size_wh.0, size_wh.1),
        }
    }

    pub fn resize(&mut self, new_width: usize, new_height: usize) -> &mut Self {
        self.visited.resize(new_width, new_height);
        return self;
    }

    pub fn clear(&mut self) -> &mut Self {
        self.visited.fill(VisitState::Unvisited);
        return self;
    }

    #[allow(unused)]
    pub fn dimensions(&self) -> (usize, usize) {
        return self.visited.dimensions();
    }

    pub fn set_visited(&mut self, point_index: pidx) {
        self.visited.set_state(VisitState::Visited, point_index);
    }

    pub fn is_visited(&self, point_index: pidx) -> bool {
        return self.visited.read(point_index) == VisitState::Visited;
    }

    pub fn is_searched(&self, point_index: pidx) -> bool {
        return self.visited.read(point_index) != VisitState::Unvisited;
    }

    pub fn get_neighbours_visited(&self, point_index: pidx) -> Vec<pidx> {
        let mut vv_nbs = Vec::<pidx>::with_capacity(4);
        for nb_pt in self._get_4way_steps(point_index) {
            if self.is_visited(nb_pt) && nb_pt != point_index {
                vv_nbs.push(nb_pt);
            }
        }
        return vv_nbs;
    }

    pub fn get_neighbours_unvisited(&mut self, point_index: pidx) -> Vec<pidx> {
        let mut uv_nbs = Vec::<pidx>::with_capacity(4);
        for nb_pt in self._get_4way_steps(point_index) {
            if !self.is_visited(nb_pt) && nb_pt != point_index {
                uv_nbs.push(nb_pt);
            }
        }
        return uv_nbs;
    }

    pub fn get_neighbours_unsearched(&mut self, point_index: pidx) -> Vec<pidx> {
        let mut uv_nbs = Vec::<pidx>::with_capacity(4);
        for nb_pt in self._get_4way_steps(point_index) {
            if !self.is_searched(nb_pt) && nb_pt != point_index {
                uv_nbs.push(nb_pt);
                self.visited.set_state(VisitState::Searched, nb_pt);
            }
        }
        return uv_nbs;
    }

    fn _get_4way_steps(&self, point_index: pidx) -> [pidx; 4] {
        let (x_mid, y_mid) = xy_from_index(point_index, self.visited.width);

        let x1 = x_mid.saturating_sub(1);
        let x2 = (x_mid + 1).min(self.visited.width - 1);
        let y1 = y_mid.saturating_sub(1);
        let y2 = (y_mid + 1).min(self.visited.height - 1);

        // Up, left, right, down
        return [(x_mid, y1), (x1, y_mid), (x2, y_mid), (x_mid, y2)]
            .map(|(x, y)| index_from_xy(x, y, self.visited.width));
    }

    #[allow(unused)]
    pub fn debug_get_total_visited(&self) -> usize {
        let mut count: usize = 0;
        for pt in self.visited.iter_1d() {
            if *pt == VisitState::Visited {
                count += 1;
            }
        }
        return count;
    }

    #[allow(unused)]
    pub fn debug_get_total_searched(&self) -> usize {
        let mut count: usize = 0;
        for pt in self.visited.iter_1d() {
            if *pt != VisitState::Unvisited {
                count += 1;
            }
        }
        return count;
    }

    #[allow(unused)]
    pub fn debug_get_total_unsearched(&self) -> usize {
        let mut count: usize = 0;
        for pt in self.visited.iter_1d() {
            if !(*pt == VisitState::Searched || *pt == VisitState::Visited) {
                count += 1;
            }
        }
        return count;
    }
}

// --------------------------------------------------------------------------------------------------------------------
// %% Functions

pub fn xy_from_index(point_index: pidx, grid_width: usize) -> (usize, usize) {
    return (point_index % grid_width, point_index / grid_width);
}

pub fn xy_from_index_u32(point_index: pidx_u32, grid_width: u32) -> (u32, u32) {
    return (point_index % grid_width, point_index / grid_width);
}

pub fn index_from_xy(x: usize, y: usize, grid_width: usize) -> pidx {
    return y * grid_width + x;
}

pub fn random_boundary_index(width: usize, height: usize) -> pidx {
    let (max_x, max_y) = (width - 1, height - 1);
    let (xmid, ymid) = (max_x / 2, max_y / 2);
    let (x, y) = match random_range(0usize..9usize) {
        0 => (0, 0),                             // TL
        1 => (xmid, 0),                          // Top-mid
        2 => (max_x, 0),                         // TR
        3 => (max_x, ymid),                      // Right-mid
        4 => (max_x, max_y),                     // BR
        5 => (xmid, max_y),                      // Bottom-mid
        6 => (0, max_y),                         // BL
        7 => (0, ymid),                          // Left-Mid
        8 => (xmid, ymid),                       // Exact-mid
        _ => panic!("Unexpected random index!"), // Never happens
    };
    return index_from_xy(x, y, width);
}

pub fn random_xy_index(width: usize, height: usize) -> pidx {
    let x = random_range(0..width);
    let y = random_range(0..height);
    return index_from_xy(x, y, width);
}
