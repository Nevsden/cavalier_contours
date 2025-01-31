use std::{
    ops::{Index, IndexMut},
    slice::Windows,
};

use static_aabb2d_index::{StaticAABB2DIndex, StaticAABB2DIndexBuilder, AABB};

use crate::{
    base_math::angle_from_bulge,
    core_math::{
        angle, arc_seg_bounding_box, delta_angle, dist_squared, is_left, is_left_or_equal,
        point_on_circle, seg_arc_radius_and_center, seg_closest_point,
        seg_fast_approx_bounding_box, seg_length,
    },
    polyline_offset, PlineVertex, Real, Vector2,
};

#[derive(Debug, Clone)]
pub struct Polyline<T = f64> {
    vertex_data: Vec<PlineVertex<T>>,
    is_closed: bool,
}

impl<T> Polyline<T>
where
    T: Real,
{
    /// Create a new empty [Polyline] with `is_closed` set to false.
    pub fn new() -> Self {
        Polyline {
            vertex_data: Vec::new(),
            is_closed: false,
        }
    }

    /// Create a new empty [Polyline] with `is_closed` set to true.
    pub fn new_closed() -> Self {
        Polyline {
            vertex_data: Vec::new(),
            is_closed: true,
        }
    }

    /// Construct a new empty [Polyline] with `is_closed` set to false and some reserved capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Polyline {
            vertex_data: Vec::with_capacity(capacity),
            is_closed: false,
        }
    }

    /// Returns the number of vertexes currently in the polyline.
    pub fn len(&self) -> usize {
        self.vertex_data.len()
    }

    /// Reserves capacity for at least `additional` more elements.
    pub fn reserve(&mut self, additional: usize) {
        self.vertex_data.reserve(additional);
    }

    /// Add a vertex to the polyline by giving the `x`, `y`, and `bulge` values of the vertex.
    pub fn add(&mut self, x: T, y: T, bulge: T) {
        self.vertex_data.push(PlineVertex::new(x, y, bulge));
    }

    /// Add vertex from array data (index 0 = x, 1 = y, 2 = bulge).
    pub fn add_from_array(&mut self, data: [T; 3]) {
        self.add(data[0], data[1], data[2]);
    }

    /// Add a vertex if it's position is not fuzzy equal to the last vertex in the polyline.
    ///
    /// If the vertex position is fuzzy equal then just update the bulge of the last vertex with
    /// the bulge given.
    pub(crate) fn add_or_replace(&mut self, x: T, y: T, bulge: T, pos_equal_eps: T) {
        let ln = self.len();
        if ln == 0 {
            self.add(x, y, bulge);
            return;
        }

        let last_vert = &mut self.vertex_data[ln - 1];
        if last_vert.x.fuzzy_eq_eps(x, pos_equal_eps) && last_vert.y.fuzzy_eq_eps(y, pos_equal_eps)
        {
            last_vert.bulge = bulge;
            return;
        }

        self.add(x, y, bulge);
    }

    /// Add a vertex if it's position is not fuzzy equal to the last vertex in the polyline.
    ///
    /// If the vertex position is fuzzy equal then just update the bulge of the last vertex with
    /// the bulge given.
    pub(crate) fn add_or_replace_vertex(&mut self, vertex: PlineVertex<T>, pos_equal_eps: T) {
        self.add_or_replace(vertex.x, vertex.y, vertex.bulge, pos_equal_eps)
    }

    /// Returns the next wrapping vertex index for the polyline.
    ///
    /// If `i + 1 >= self.len()` then 0 is returned, otherwise `i + 1` is returned.
    pub fn next_wrapping_index(&self, i: usize) -> usize {
        let next = i + 1;
        if next >= self.len() {
            0
        } else {
            next
        }
    }

    /// Returns the previous wrapping vertex index for the polyline.
    ///
    /// If `i == 0` then `self.len() - 1` is returned, otherwise `i - 1` is returned.
    pub fn prev_wrapping_index(&self, i: usize) -> usize {
        if i == 0 {
            self.len() - 1
        } else {
            i - 1
        }
    }

    /// Add a vertex to the polyline by giving a [PlineVertex](crate::PlineVertex).
    pub fn add_vertex(&mut self, vertex: PlineVertex<T>) {
        self.vertex_data.push(vertex);
    }

    /// Copy all vertexes from other to the end of this polyline.
    pub fn extend_vertexes(&mut self, other: &Polyline<T>) {
        self.vertex_data.extend(other.vertex_data.iter());
    }

    /// Remove vertex at index.
    pub fn remove(&mut self, index: usize) {
        self.vertex_data.remove(index);
    }

    /// Remove last vertex.
    pub fn remove_last(&mut self) {
        self.remove(self.len() - 1);
    }

    /// Clear all vertexes.
    pub fn clear(&mut self) {
        self.vertex_data.clear();
    }

    /// Returns true if the polyline is closed, false if it is open.
    pub fn is_closed(&self) -> bool {
        self.is_closed
    }

    /// Allows modifying whether the polyline is closed or not.
    pub fn set_is_closed(&mut self, is_closed: bool) {
        self.is_closed = is_closed;
    }

    pub fn last(&self) -> Option<&PlineVertex<T>> {
        self.vertex_data.last()
    }

    pub fn last_mut(&mut self) -> Option<&mut PlineVertex<T>> {
        self.vertex_data.last_mut()
    }

    /// Set the vertex data at a given index of the polyline.
    pub fn set_vertex(&mut self, index: usize, x: T, y: T, bulge: T) {
        self.vertex_data[index].x = x;
        self.vertex_data[index].y = y;
        self.vertex_data[index].bulge = bulge;
    }

    /// Fuzzy equal comparison with another polyline using `fuzzy_epsilon` given.
    pub fn fuzzy_eq_eps(&self, other: &Self, fuzzy_epsilon: T) -> bool {
        self.vertex_data
            .iter()
            .zip(&other.vertex_data)
            .all(|(v1, v2)| v1.fuzzy_eq_eps(*v2, fuzzy_epsilon))
    }

    /// Fuzzy equal comparison with another vertex using T::fuzzy_epsilon().
    pub fn fuzzy_eq(&self, other: &Self) -> bool {
        self.fuzzy_eq_eps(other, T::fuzzy_epsilon())
    }

    /// Invert/reverse the direction of the polyline in place.
    ///
    /// This method works by simply reversing the order of the vertexes,
    /// and then shifting (by 1 position) and inverting the sign of all the bulge values.
    /// E.g. after reversing the vertex the bulge at index 0 becomes negative bulge at index 1.
    /// The end result for a closed polyline is the direction will be changed
    /// from clockwise to counter clockwise or vice versa.
    pub fn invert_direction(&mut self) {
        let ln = self.len();
        if ln < 2 {
            return;
        }

        self.vertex_data.reverse();

        let first_bulge = self[0].bulge;
        for i in 1..ln {
            self[i - 1].bulge = -self[i].bulge;
        }

        if self.is_closed {
            self[ln - 1].bulge = -first_bulge;
        }
    }

    /// Uniformly scale the polyline in the xy plane by `scale_factor`.
    ///
    /// # Examples
    ///
    /// ```
    /// # use cavalier_contours::*;
    /// let mut polyline = Polyline::new();
    /// polyline.add(2.0, 2.0, 0.5);
    /// polyline.add(4.0, 4.0, 1.0);
    /// polyline.scale(2.0);
    /// let mut expected = Polyline::new();
    /// expected.add(4.0, 4.0, 0.5);
    /// expected.add(8.0, 8.0, 1.0);
    /// assert!(polyline.fuzzy_eq(&expected));
    /// ```
    pub fn scale(&mut self, scale_factor: T) {
        for v in self.iter_mut() {
            v.x = v.x * scale_factor;
            v.y = v.y * scale_factor;
        }
    }

    /// Translate the polyline by some `x_offset` and `y_offset`.
    ///
    /// # Examples
    ///
    /// ```
    /// # use cavalier_contours::*;
    /// let mut polyline = Polyline::new();
    /// polyline.add(2.0, 2.0, 0.5);
    /// polyline.add(4.0, 4.0, 1.0);
    /// polyline.translate(-3.0, 1.0);
    /// let mut expected = Polyline::new();
    /// expected.add(-1.0, 3.0, 0.5);
    /// expected.add(1.0, 5.0, 1.0);
    /// assert!(polyline.fuzzy_eq(&expected));
    /// ```
    pub fn translate(&mut self, x_offset: T, y_offset: T) {
        for v in self.iter_mut() {
            v.x = v.x + x_offset;
            v.y = v.y + y_offset;
        }
    }

    /// Compute the XY extents of the polyline.
    ///
    /// Returns `None` if polyline is empty. If polyline has only one vertex then
    /// `min_x = max_x = polyline[0].x` and `min_y = max_y = polyline[0].y`.
    ///
    /// # Examples
    ///
    /// ```
    /// # use cavalier_contours::*;
    /// let mut polyline = Polyline::new();
    /// assert_eq!(polyline.extents(), None);
    /// polyline.add(1.0, 1.0, 1.0);
    /// let pt_extents = polyline.extents().unwrap();
    /// assert!(pt_extents.min_x.fuzzy_eq(1.0));
    /// assert!(pt_extents.min_y.fuzzy_eq(1.0));
    /// assert!(pt_extents.max_x.fuzzy_eq(1.0));
    /// assert!(pt_extents.max_y.fuzzy_eq(1.0));
    ///
    /// polyline.add(3.0, 1.0, 1.0);
    /// let extents = polyline.extents().unwrap();
    /// assert!(extents.min_x.fuzzy_eq(1.0));
    /// assert!(extents.min_y.fuzzy_eq(0.0));
    /// assert!(extents.max_x.fuzzy_eq(3.0));
    /// assert!(extents.max_y.fuzzy_eq(1.0));
    ///
    /// polyline.set_is_closed(true);
    /// let extents = polyline.extents().unwrap();
    /// assert!(extents.min_x.fuzzy_eq(1.0));
    /// assert!(extents.min_y.fuzzy_eq(0.0));
    /// assert!(extents.max_x.fuzzy_eq(3.0));
    /// assert!(extents.max_y.fuzzy_eq(2.0));
    /// ```
    pub fn extents(&self) -> Option<AABB<T>> {
        if self.len() == 0 {
            return None;
        }

        let mut result = AABB::new(self[0].x, self[0].y, self[0].x, self[0].y);

        for (v1, v2) in self.iter_segments() {
            if v1.bulge_is_zero() {
                // line segment, just look at end of line point
                if v2.x < result.min_x {
                    result.min_x = v2.x;
                } else if v2.x > result.max_x {
                    result.max_x = v2.x;
                }

                if v2.y < result.min_y {
                    result.min_y = v2.y;
                } else if v2.y > result.max_y {
                    result.max_y = v2.y;
                }

                continue;
            }
            // else arc segment
            let arc_extents = arc_seg_bounding_box(v1, v2);

            result.min_x = num_traits::real::Real::min(result.min_x, arc_extents.min_x);
            result.min_y = num_traits::real::Real::min(result.min_y, arc_extents.min_y);
            result.max_x = num_traits::real::Real::max(result.max_x, arc_extents.max_x);
            result.max_y = num_traits::real::Real::max(result.max_y, arc_extents.max_y);
        }

        Some(result)
    }

    pub fn create_approx_spatial_index(&self) -> Option<StaticAABB2DIndex<T>> {
        let ln = self.len();
        if ln < 2 {
            return None;
        }

        let seg_count = if self.is_closed { ln } else { ln - 1 };

        let mut builder = StaticAABB2DIndexBuilder::new(seg_count);

        for i in 0..ln - 1 {
            let approx_aabb = seg_fast_approx_bounding_box(self[i], self[i + 1]);
            builder.add(
                approx_aabb.min_x,
                approx_aabb.min_y,
                approx_aabb.max_x,
                approx_aabb.max_y,
            );
        }

        if self.is_closed {
            // add final segment from last to first
            let approx_aabb = seg_fast_approx_bounding_box(*self.last().unwrap(), self[0]);
            builder.add(
                approx_aabb.min_x,
                approx_aabb.min_y,
                approx_aabb.max_x,
                approx_aabb.max_y,
            );
        }

        builder.build().ok()
    }

    /// Visit all the polyline segments (represented as polyline vertex pairs) with a function/closure.
    ///
    /// This is equivalent to [Polyline::iter_segments] but uses a visiting function rather than an iterator.
    pub fn visit_segments<F>(&self, visitor: &mut F)
    where
        F: FnMut(PlineVertex<T>, PlineVertex<T>) -> bool,
    {
        let ln = self.vertex_data.len();
        if ln < 2 {
            return;
        }

        if self.is_closed {
            let v1 = self.vertex_data[ln - 1];
            let v2 = self.vertex_data[0];
            if !visitor(v1, v2) {
                return;
            }
        }

        let mut windows = self.vertex_data.windows(2);
        while let Some(&[v1, v2]) = windows.next() {
            if !visitor(v1, v2) {
                break;
            }
        }
    }

    /// Iterate through all the vertexes in the polyline.
    pub fn iter(&self) -> impl Iterator<Item = &PlineVertex<T>> {
        self.vertex_data.iter()
    }

    /// Iterate through all the vertexes in the polyline as mutable references.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut PlineVertex<T>> {
        self.vertex_data.iter_mut()
    }

    /// Iterate through all the polyline segments (represented as polyline vertex pairs).
    ///
    /// This is equivalent to [Polyline::visit_segments] but returns an iterator rather than accepting a function.
    pub fn iter_segments<'a>(
        &'a self,
    ) -> impl Iterator<Item = (PlineVertex<T>, PlineVertex<T>)> + 'a {
        PlineSegIterator::new(&self)
    }

    /// Iterate through all the polyline segment vertex positional indexes.
    ///
    /// Segments are represented by polyline vertex pairs, for each vertex there is
    /// an associated positional index in the polyline, this method iterates through
    /// those positional indexes as segment pairs.
    pub fn iter_segment_indexes(&self) -> impl Iterator<Item = (usize, usize)> {
        PlineSegIndexIterator::new(self.vertex_data.len(), self.is_closed)
    }

    pub fn parallel_offset(
        &self,
        offset: T,
        spatial_index: Option<&StaticAABB2DIndex<T>>,
    ) -> Vec<Polyline<T>> {
        polyline_offset::parallel_offset(self, offset, spatial_index, None)
    }

    /// Compute the closed signed area of the polyline.
    ///
    /// If [Polyline::is_closed] is false (open polyline) then 0.0 is always returned.
    /// The area is signed such that if the polyline direction is counter clockwise
    /// then the area is positive, otherwise it is negative.
    ///
    /// # Examples
    ///
    /// ```
    /// # use cavalier_contours::*;
    /// let mut polyline: Polyline = Polyline::new();
    /// assert!(polyline.area().fuzzy_eq(0.0));
    /// polyline.add(1.0, 1.0, 1.0);
    /// assert!(polyline.area().fuzzy_eq(0.0));
    ///
    /// polyline.add(3.0, 1.0, 1.0);
    /// // polyline is still open so area is 0
    /// assert!(polyline.area().fuzzy_eq(0.0));
    /// polyline.set_is_closed(true);
    /// assert!(polyline.area().fuzzy_eq(std::f64::consts::PI));
    /// polyline.invert_direction();
    /// assert!(polyline.area().fuzzy_eq(-std::f64::consts::PI));
    /// ```
    pub fn area(&self) -> T {
        if !self.is_closed {
            return T::zero();
        }

        // Implementation notes:
        // Using the shoelace formula (https://en.wikipedia.org/wiki/Shoelace_formula) modified to support
        // arcs defined by a bulge value. The shoelace formula returns a negative value for clockwise
        // oriented polygons and positive value for counter clockwise oriented polygons. The area of each
        // circular segment defined by arcs is then added if it is a counter clockwise arc or subtracted
        // if it is a clockwise arc. The area of the circular segments are computed by finding the area of
        // the arc sector minus the area of the triangle defined by the chord and center of circle.
        // See https://en.wikipedia.org/wiki/Circular_segment

        let mut double_total_area = T::zero();

        for (v1, v2) in self.iter_segments() {
            double_total_area = double_total_area + v1.x * v2.y - v1.y * v2.x;
            if !v1.bulge_is_zero() {
                // add arc segment area
                let b = v1.bulge.abs();
                let sweep_angle = angle_from_bulge(b);
                let triangle_base = (v2.pos() - v1.pos()).length();
                let radius = triangle_base * ((b * b + T::one()) / (T::four() * b));
                let sagitta = b * triangle_base / T::two();
                let triangle_height = radius - sagitta;
                let double_sector_area = sweep_angle * radius * radius;
                let double_triangle_area = triangle_base * triangle_height;
                let mut double_arc_area = double_sector_area - double_triangle_area;
                if v1.bulge_is_neg() {
                    double_arc_area = -double_arc_area;
                }

                double_total_area = double_total_area + double_arc_area;
            }
        }

        double_total_area / T::two()
    }

    /// Find the closest segment point on a polyline to a `point` given.
    ///
    /// If the polyline is empty then `None` is returned.
    ///
    /// # Examples
    ///
    /// ```
    /// # use cavalier_contours::*;
    /// let mut polyline: Polyline = Polyline::new();
    /// assert!(matches!(polyline.closest_point(Vector2::zero()), None));
    /// polyline.add(1.0, 1.0, 1.0);
    /// let result = polyline.closest_point(Vector2::new(1.0, 0.0)).unwrap();
    /// assert_eq!(result.seg_start_index, 0);
    /// assert!(result.seg_point.fuzzy_eq(polyline[0].pos()));
    /// assert!(result.distance.fuzzy_eq(1.0));
    /// ```
    pub fn closest_point(&self, point: Vector2<T>) -> Option<ClosestPointResult<T>> {
        if self.len() == 0 {
            return None;
        }

        let mut result = ClosestPointResult {
            seg_start_index: 0,
            seg_point: self[0].pos(),
            distance: Real::max_value(),
        };

        if self.len() == 1 {
            result.distance = (result.seg_point - point).length();
            return Some(result);
        }

        let mut dist_squared = Real::max_value();

        for (i, j) in self.iter_segment_indexes() {
            let v1 = self[i];
            let v2 = self[j];
            let cp = seg_closest_point(v1, v2, point);
            let diff_v = point - cp;
            let dist2 = diff_v.length_squared();
            if dist2 < dist_squared {
                result.seg_start_index = i;
                result.seg_point = cp;
                dist_squared = dist2;
            }
        }

        result.distance = dist_squared.sqrt();

        Some(result)
    }

    /// Returns the total path length of the polyline.
    ///
    /// # Examples
    ///
    /// ```
    /// # use cavalier_contours::*;
    /// let mut polyline: Polyline = Polyline::new();
    /// // open polyline half circle
    /// polyline.add(0.0, 0.0, 1.0);
    /// polyline.add(2.0, 0.0, 1.0);
    /// assert!(polyline.path_length().fuzzy_eq(std::f64::consts::PI));
    /// // close into full circle
    /// polyline.set_is_closed(true);
    /// assert!(polyline.path_length().fuzzy_eq(2.0 * std::f64::consts::PI));
    /// ```
    pub fn path_length(&self) -> T {
        self.iter_segments()
            .fold(T::zero(), |acc, (v1, v2)| acc + seg_length(v1, v2))
    }

    /// Helper function for processing a line segment when computing the winding number.
    fn process_line_winding(v1: PlineVertex<T>, v2: PlineVertex<T>, point: Vector2<T>) -> i32 {
        let mut result = 0;
        if v1.y <= point.y {
            if v2.y > point.y && is_left(v1.pos(), v2.pos(), point) {
                // left and upward crossing
                result += 1;
            }
        } else if v2.y <= point.y && !is_left(v1.pos(), v2.pos(), point) {
            // right an downward crossing
            result -= 1;
        }

        result
    }

    /// Helper function for processing an arc segment when computing the winding number.
    fn process_arc_winding(v1: PlineVertex<T>, v2: PlineVertex<T>, point: Vector2<T>) -> i32 {
        let is_ccw = v1.bulge_is_pos();
        let point_is_left = if is_ccw {
            is_left(v1.pos(), v2.pos(), point)
        } else {
            is_left_or_equal(v1.pos(), v2.pos(), point)
        };

        let dist_to_arc_center_less_than_radius = || {
            let (arc_radius, arc_center) = seg_arc_radius_and_center(v1, v2);
            let dist2 = dist_squared(arc_center, point);
            dist2 < arc_radius * arc_radius
        };

        let mut result = 0;

        if v1.y <= point.y {
            if v2.y > point.y {
                // upward crossing of arc chord
                if is_ccw {
                    if point_is_left {
                        // counter clockwise arc left of chord
                        result += 1;
                    } else {
                        // counter clockwise arc right of chord
                        if dist_to_arc_center_less_than_radius() {
                            result += 1;
                        }
                    }
                } else {
                    if point_is_left {
                        // clockwise arc left of chord
                        if !dist_to_arc_center_less_than_radius() {
                            result += 1;
                        }
                        // else clockwise arc right of chord, no crossing
                    }
                }
            } else {
                // not crossing arc chord and chord is below, check if point is inside arc sector
                if is_ccw && !point_is_left {
                    if v2.x < point.x && point.x < v1.x && dist_to_arc_center_less_than_radius() {
                        result += 1;
                    }
                } else if !is_ccw && point_is_left {
                    if v1.x < point.x && point.x < v2.x && dist_to_arc_center_less_than_radius() {
                        result -= 1;
                    }
                }
            }
        } else if v2.y <= point.y {
            // downward crossing of arc chord
            if is_ccw {
                if !point_is_left {
                    // counter clockwise arc right of chord
                    if dist_to_arc_center_less_than_radius() {
                        result -= 1;
                    }
                }
            // else counter clockwise arc left of chord, no crossing
            } else {
                if point_is_left {
                    // clockwise arc left of chord
                    if dist_to_arc_center_less_than_radius() {
                        result -= 1;
                    }
                } else {
                    // clockwise arc right of chord
                    result -= 1;
                }
            }
        } else {
            // not crossing arc chord and chord is above, check if point is inside arc sector
            if is_ccw && !point_is_left {
                if v1.x < point.x && point.x < v2.x && dist_to_arc_center_less_than_radius() {
                    result += 1;
                }
            } else {
                if v2.x < point.x && point.x < v1.x && dist_to_arc_center_less_than_radius() {
                    result -= 1;
                }
            }
        }

        result
    }

    /// Calculate the winding number for a `point` relative to the polyline.
    ///
    /// The winding number calculates the number of turns/windings around a point
    /// that the polyline path makes. For a closed polyline without self intersects
    /// there are only three possibilities:
    ///
    /// * -1 (winds around point clockwise)
    /// * 0 (point is outside the polyline)
    /// * 1 (winds around the point counter clockwise).
    ///
    /// This function always returns 0 if polyline [Polyline::is_closed] is false.
    ///
    /// If the point lies directly on top of one of the polyline segments the result
    /// is not defined.
    ///
    /// # Examples
    ///
    /// ### Polyline without self intersects
    ///
    /// ```
    /// # use cavalier_contours::*;
    /// let mut polyline: Polyline = Polyline::new_closed();
    /// polyline.add(0.0, 0.0, 1.0);
    /// polyline.add(2.0, 0.0, 1.0);
    /// assert_eq!(polyline.winding_number(Vector2::new(1.0, 0.0)), 1);
    /// assert_eq!(polyline.winding_number(Vector2::new(0.0, 2.0)), 0);
    /// polyline.invert_direction();
    /// assert_eq!(polyline.winding_number(Vector2::new(1.0, 0.0)), -1);
    /// ```
    ///
    /// ### Multiple windings with self intersecting polyline
    ///
    /// ```
    /// # use cavalier_contours::*;
    /// let mut polyline: Polyline = Polyline::new_closed();
    /// polyline.add(0.0, 0.0, 1.0);
    /// polyline.add(2.0, 0.0, 1.0);
    /// polyline.add(0.0, 0.0, 1.0);
    /// polyline.add(4.0, 0.0, 1.0);
    /// assert_eq!(polyline.winding_number(Vector2::new(1.0, 0.0)), 2);
    /// assert_eq!(polyline.winding_number(Vector2::new(-1.0, 0.0)), 0);
    /// polyline.invert_direction();
    /// assert_eq!(polyline.winding_number(Vector2::new(1.0, 0.0)), -2);
    /// ```
    pub fn winding_number(&self, point: Vector2<T>) -> i32 {
        if !self.is_closed || self.len() < 2 {
            return 0;
        }

        let mut winding = 0;

        for (v1, v2) in self.iter_segments() {
            if v1.bulge_is_zero() {
                winding += Self::process_line_winding(v1, v2, point);
            } else {
                winding += Self::process_arc_winding(v1, v2, point);
            }
        }

        winding
    }

    /// Returns a new polyline with all arc segments converted to line segments with some `error_distance` or None
    /// if T fails to cast to or from usize.
    ///
    /// `error_distance` is the maximum distance from any line segment to the arc it is approximating.
    /// Line segments are circumscribed by the arc (all line end points lie on the arc path).
    pub fn arcs_to_approx_lines(&self, error_distance: T) -> Option<Self> {
        let mut result = Polyline::new();
        result.set_is_closed(self.is_closed);

        // catch case where length is 0 since we may index into the last vertex later
        if self.len() == 0 {
            return Some(result);
        }

        let abs_error = error_distance.abs();

        for (v1, v2) in self.iter_segments() {
            if v1.bulge_is_zero() {
                result.add_vertex(v1);
                continue;
            }

            let (arc_radius, arc_center) = seg_arc_radius_and_center(v1, v2);
            if arc_radius.fuzzy_lt(error_distance) {
                result.add(v1.x, v1.y, T::zero());
                continue;
            }

            let start_angle = angle(arc_center, v1.pos());
            let end_angle = angle(arc_center, v2.pos());
            let angle_diff = delta_angle(start_angle, end_angle).abs();

            let seg_sub_angle = T::two() * (T::one() - abs_error / arc_radius).acos().abs();
            let seg_count = (angle_diff / seg_sub_angle).ceil();
            // create angle offset such that all lines have an equal part of the arc
            let seg_angle_offset = if v1.bulge_is_neg() {
                -angle_diff / seg_count
            } else {
                angle_diff / seg_count
            };

            // add start vertex
            result.add(v1.x, v1.y, T::zero());
            let usize_count = seg_count.to_usize()?;
            // add all vertex points along arc
            for i in 1..usize_count {
                let angle_pos = T::from(i)?;
                let angle = angle_pos * seg_angle_offset + start_angle;
                let pos = point_on_circle(arc_radius, arc_center, angle);
                result.add(pos.x, pos.y, T::zero());
            }
        }

        if !self.is_closed {
            // add the final missing vertex in the case that the polyline is not closed
            result.add_vertex(self[self.len() - 1]);
        }

        Some(result)
    }
}

/// Result from calling [Polyline::closest_point].
#[derive(Debug, Copy, Clone)]
pub struct ClosestPointResult<T>
where
    T: Real,
{
    /// The start vertex index of the closest segment.
    pub seg_start_index: usize,
    /// The closest point on the closest segment.
    pub seg_point: Vector2<T>,
    /// The distance between the points.
    pub distance: T,
}

impl<T> Index<usize> for Polyline<T>
where
    T: Real,
{
    type Output = PlineVertex<T>;

    #[inline]
    fn index(&self, index: usize) -> &Self::Output {
        &self.vertex_data[index]
    }
}

impl<T> IndexMut<usize> for Polyline<T>
where
    T: Real,
{
    #[inline]
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.vertex_data[index]
    }
}

struct PlineSegIterator<'a, T>
where
    T: Real,
{
    polyline: &'a Polyline<T>,
    vertex_windows: Windows<'a, PlineVertex<T>>,
    is_closed_first_pass: bool,
}

impl<'a, T> PlineSegIterator<'a, T>
where
    T: Real,
{
    fn new(polyline: &'a Polyline<T>) -> PlineSegIterator<'a, T> {
        let vertex_windows = polyline.vertex_data.windows(2);
        let is_closed_first_pass = if polyline.vertex_data.len() < 2 {
            false
        } else {
            polyline.is_closed
        };
        PlineSegIterator {
            polyline,
            vertex_windows,
            is_closed_first_pass,
        }
    }
}

impl<'a, T> Iterator for PlineSegIterator<'a, T>
where
    T: Real,
{
    type Item = (PlineVertex<T>, PlineVertex<T>);

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if self.is_closed_first_pass {
            self.is_closed_first_pass = false;
            let ln = self.polyline.vertex_data.len();
            return Some((self.polyline[ln - 1], self.polyline[0]));
        }

        if let Some(&[v1, v2]) = self.vertex_windows.next() {
            Some((v1, v2))
        } else {
            None
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        if self.is_closed_first_pass {
            let ln = self.polyline.vertex_data.len();
            (ln, Some(ln))
        } else {
            self.vertex_windows.size_hint()
        }
    }
}

struct PlineSegIndexIterator {
    pos: usize,
    remaining: usize,
    is_closed_first_pass: bool,
}

impl PlineSegIndexIterator {
    fn new(vertex_count: usize, is_closed: bool) -> PlineSegIndexIterator {
        let remaining = if vertex_count < 2 {
            0
        } else if is_closed {
            vertex_count
        } else {
            vertex_count - 1
        };
        PlineSegIndexIterator {
            pos: 1,
            remaining,
            is_closed_first_pass: is_closed,
        }
    }
}

impl Iterator for PlineSegIndexIterator {
    type Item = (usize, usize);

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }

        self.remaining -= 1;

        if self.is_closed_first_pass {
            self.is_closed_first_pass = false;
            return Some((self.remaining, 0));
        }

        let pos = self.pos;
        self.pos += 1;
        Some((pos - 1, pos))
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

#[cfg(test)]
mod tests {

    use std::f64::consts::PI;

    use super::*;
    use crate::FuzzyEq;

    #[test]
    fn iter_segments() {
        let mut polyline = Polyline::<f64>::new();
        assert_eq!(polyline.iter_segments().size_hint(), (0, Some(0)));
        assert_eq!(polyline.iter_segments().collect::<Vec<_>>().len(), 0);

        polyline.add(1.0, 2.0, 0.3);
        assert_eq!(polyline.iter_segments().size_hint(), (0, Some(0)));
        assert_eq!(polyline.iter_segments().collect::<Vec<_>>().len(), 0);

        polyline.add(4.0, 5.0, 0.6);
        assert_eq!(polyline.iter_segments().size_hint(), (1, Some(1)));
        let one_seg = polyline.iter_segments().collect::<Vec<_>>();
        assert_eq!(one_seg.len(), 1);
        assert_fuzzy_eq!(one_seg[0].0, PlineVertex::new(1.0, 2.0, 0.3));
        assert_fuzzy_eq!(one_seg[0].1, PlineVertex::new(4.0, 5.0, 0.6));

        polyline.set_is_closed(true);
        assert_eq!(polyline.iter_segments().size_hint(), (2, Some(2)));
        let two_seg = polyline.iter_segments().collect::<Vec<_>>();
        assert_eq!(two_seg.len(), 2);
        assert_fuzzy_eq!(two_seg[0].0, PlineVertex::new(4.0, 5.0, 0.6));
        assert_fuzzy_eq!(two_seg[0].1, PlineVertex::new(1.0, 2.0, 0.3));
        assert_fuzzy_eq!(two_seg[1].0, PlineVertex::new(1.0, 2.0, 0.3));
        assert_fuzzy_eq!(two_seg[1].1, PlineVertex::new(4.0, 5.0, 0.6));

        polyline.add(0.5, 0.5, 0.5);
        assert_eq!(polyline.iter_segments().size_hint(), (3, Some(3)));
        let three_seg = polyline.iter_segments().collect::<Vec<_>>();
        assert_fuzzy_eq!(three_seg[0].0, PlineVertex::new(0.5, 0.5, 0.5));
        assert_fuzzy_eq!(three_seg[0].1, PlineVertex::new(1.0, 2.0, 0.3));
        assert_fuzzy_eq!(three_seg[1].0, PlineVertex::new(1.0, 2.0, 0.3));
        assert_fuzzy_eq!(three_seg[1].1, PlineVertex::new(4.0, 5.0, 0.6));
        assert_fuzzy_eq!(three_seg[2].0, PlineVertex::new(4.0, 5.0, 0.6));
        assert_fuzzy_eq!(three_seg[2].1, PlineVertex::new(0.5, 0.5, 0.5));

        polyline.set_is_closed(false);
        assert_eq!(polyline.iter_segments().size_hint(), (2, Some(2)));
        let two_seg_open = polyline.iter_segments().collect::<Vec<_>>();
        assert_fuzzy_eq!(two_seg_open[0].0, PlineVertex::new(1.0, 2.0, 0.3));
        assert_fuzzy_eq!(two_seg_open[0].1, PlineVertex::new(4.0, 5.0, 0.6));
        assert_fuzzy_eq!(two_seg_open[1].0, PlineVertex::new(4.0, 5.0, 0.6));
        assert_fuzzy_eq!(two_seg_open[1].1, PlineVertex::new(0.5, 0.5, 0.5));
    }

    #[test]
    fn iter_segment_indexes() {
        let mut polyline = Polyline::<f64>::new();
        assert_eq!(polyline.iter_segment_indexes().size_hint(), (0, Some(0)));
        assert_eq!(polyline.iter_segment_indexes().collect::<Vec<_>>().len(), 0);

        polyline.add(1.0, 2.0, 0.3);
        assert_eq!(polyline.iter_segment_indexes().size_hint(), (0, Some(0)));
        assert_eq!(polyline.iter_segment_indexes().collect::<Vec<_>>().len(), 0);

        polyline.add(4.0, 5.0, 0.6);
        assert_eq!(polyline.iter_segment_indexes().size_hint(), (1, Some(1)));
        let one_seg = polyline.iter_segment_indexes().collect::<Vec<_>>();
        assert_eq!(one_seg, vec![(0, 1)]);

        polyline.set_is_closed(true);
        assert_eq!(polyline.iter_segment_indexes().size_hint(), (2, Some(2)));
        let two_seg = polyline.iter_segment_indexes().collect::<Vec<_>>();
        assert_eq!(two_seg, vec![(1, 0), (0, 1)]);

        polyline.add(0.5, 0.5, 0.5);
        assert_eq!(polyline.iter_segment_indexes().size_hint(), (3, Some(3)));
        let three_seg = polyline.iter_segment_indexes().collect::<Vec<_>>();
        assert_eq!(three_seg, vec![(2, 0), (0, 1), (1, 2)]);

        polyline.set_is_closed(false);
        assert_eq!(polyline.iter_segment_indexes().size_hint(), (2, Some(2)));
        let two_seg_open = polyline.iter_segment_indexes().collect::<Vec<_>>();
        assert_eq!(two_seg_open, vec![(0, 1), (1, 2)]);
    }

    #[test]
    fn invert_direction() {
        let mut polyline = Polyline::new_closed();
        polyline.add(0.0, 0.0, 0.1);
        polyline.add(2.0, 0.0, 0.2);
        polyline.add(2.0, 2.0, 0.3);
        polyline.add(0.0, 2.0, 0.4);

        polyline.invert_direction();

        assert_fuzzy_eq!(polyline[0], PlineVertex::new(0.0, 2.0, -0.3));
        assert_fuzzy_eq!(polyline[1], PlineVertex::new(2.0, 2.0, -0.2));
        assert_fuzzy_eq!(polyline[2], PlineVertex::new(2.0, 0.0, -0.1));
        assert_fuzzy_eq!(polyline[3], PlineVertex::new(0.0, 0.0, -0.4));
    }

    #[test]
    fn area() {
        {
            let mut circle = Polyline::new_closed();
            circle.add(0.0, 0.0, 1.0);
            circle.add(2.0, 0.0, 1.0);
            assert_fuzzy_eq!(circle.area(), PI);
            circle.invert_direction();
            assert_fuzzy_eq!(circle.area(), -PI);
        }

        {
            let mut half_circle = Polyline::new_closed();
            half_circle.add(0.0, 0.0, -1.0);
            half_circle.add(2.0, 0.0, 0.0);
            assert_fuzzy_eq!(half_circle.area(), -0.5 * PI);
            half_circle.invert_direction();
            assert_fuzzy_eq!(half_circle.area(), 0.5 * PI);
        }

        {
            let mut square = Polyline::new_closed();
            square.add(0.0, 0.0, 0.0);
            square.add(2.0, 0.0, 0.0);
            square.add(2.0, 2.0, 0.0);
            square.add(0.0, 2.0, 0.0);
            assert_fuzzy_eq!(square.area(), 4.0);
            square.invert_direction();
            assert_fuzzy_eq!(square.area(), -4.0);
        }

        {
            let mut open_polyline = Polyline::new();
            open_polyline.add(0.0, 0.0, 0.0);
            open_polyline.add(2.0, 0.0, 0.0);
            open_polyline.add(2.0, 2.0, 0.0);
            open_polyline.add(0.0, 2.0, 0.0);
            assert_fuzzy_eq!(open_polyline.area(), 0.0);
            open_polyline.invert_direction();
            assert_fuzzy_eq!(open_polyline.area(), 0.0);
        }

        {
            let empty_open_polyline = Polyline::<f64>::new();
            assert_fuzzy_eq!(empty_open_polyline.area(), 0.0);
        }

        {
            let empty_closed_polyline = Polyline::<f64>::new_closed();
            assert_fuzzy_eq!(empty_closed_polyline.area(), 0.0);
        }

        {
            let mut one_vertex_open_polyline = Polyline::<f64>::new();
            one_vertex_open_polyline.add(1.0, 1.0, 0.0);
            assert_fuzzy_eq!(one_vertex_open_polyline.area(), 0.0);
        }

        {
            let mut one_vertex_closed_polyline = Polyline::<f64>::new_closed();
            one_vertex_closed_polyline.add(1.0, 1.0, 0.0);
            assert_fuzzy_eq!(one_vertex_closed_polyline.area(), 0.0);
        }
    }
}
