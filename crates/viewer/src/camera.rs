/// Smooth, cursor-anchored navigation in document coordinates.
#[derive(Clone, Debug)]
pub struct Camera {
    home: [f64; 4],
    center: [f64; 2],
    target: [f64; 2],
    zoom: f64,
    target_zoom: f64,
    anchor: [f64; 2],
    canvas: [f64; 2],
    /// Whether further navigation would collapse adjacent f64 samples.
    pub precision_limited: bool,
}
impl Camera {
    /// Start with the document's view box fitted to the canvas.
    pub fn new(view: [f32; 4]) -> Self {
        let home = view.map(f64::from);
        let center = [home[0] + home[2] * 0.5, home[1] + home[3] * 0.5];
        Self {
            home,
            center,
            target: center,
            zoom: 0.0,
            target_zoom: 0.0,
            anchor: [0.5, 0.5],
            canvas: [1., 1.],
            precision_limited: false,
        }
    }
    /// Restore the original composition and scale.
    pub fn reset(&mut self) {
        *self = Self::new(self.home.map(|v| v as f32));
    }
    /// Current magnification relative to fitting the whole document.
    pub fn magnification(&self) -> f64 {
        self.zoom.exp()
    }
    fn unit(&self, size: [f64; 2], zoom: f64) -> f64 {
        (self.home[2] / size[0].max(1.0)).max(self.home[3] / size[1].max(1.0)) * (-zoom).exp()
    }
    fn rectangle(&self, size: [f64; 2], center: [f64; 2], zoom: f64) -> [f64; 4] {
        let u = self.unit(size, zoom);
        [
            (center[0] - size[0] * u * 0.5),
            (center[1] - size[1] * u * 0.5),
            (size[0] * u),
            (size[1] * u),
        ]
    }
    fn distinct(&self, size: [f64; 2], center: [f64; 2], zoom: f64) -> bool {
        let v = self.rectangle(size, center, zoom);
        let unit = self.unit(size, zoom);
        v.iter().all(|v| v.is_finite())
            && unit.is_finite()
            && unit > 0.
            && v[2] > 0.
            && v[3] > 0.
            && unit * 0.5 > 0.
            && center[0] + unit * 0.5 != center[0]
            && center[1] + unit * 0.5 != center[1]
            && v[0] + unit * 0.5 != v[0]
            && v[1] + unit * 0.5 != v[1]
    }
    /// Whether f32 coordinate rounding exceeds one eighth of a canvas pixel.
    /// The viewer uses CPU f64 arithmetic only when required by this camera.
    pub fn requires_precision(&self, size: [f64; 2]) -> bool {
        self.precision_for(size, [size[0].max(1.) as u32, size[1].max(1.) as u32])
    }
    /// Check precision at an output resolution, including supersampling.
    /// The canvas defines the camera aspect; resolution defines the sample spacing.
    pub fn precision_for(&self, canvas: [f64; 2], resolution: [u32; 2]) -> bool {
        let v = self.precise_viewport(canvas);
        let unit =
            (v[2] / f64::from(resolution[0].max(1))).max(v[3] / f64::from(resolution[1].max(1)));
        let rounded = v.map(|v| f64::from(v as f32));
        // WGSL devices may flush f32 subnormals to zero. Switch before the
        // pixel footprint enters that range even when casts remain distinct.
        unit < f64::from(f32::MIN_POSITIVE)
            || v.iter()
                .zip(rounded)
                .any(|(a, b)| (a - b).abs() > unit * 0.125)
            || [v[0], v[1]]
                .iter()
                .any(|a| (*a as f32 + unit as f32) == *a as f32)
            || [v[0], v[1], v[0] + v[2], v[1] + v[3]]
                .iter()
                .any(|a| a.abs() * f64::from(f32::EPSILON) > unit * 0.25)
            || rounded[2] <= 0.
            || rounded[3] <= 0.
    }
    /// Change logarithmic zoom while retaining the world point under a normalized cursor.
    pub fn zoom_at(&mut self, delta: f64, anchor: [f64; 2], size: [f64; 2]) {
        if !delta.is_finite() || !anchor.iter().all(|v| v.is_finite()) {
            return;
        }
        self.anchor = anchor;
        self.canvas = size;
        let zoom = self.target_zoom + delta;
        let old = self.unit(size, self.target_zoom);
        let new = self.unit(size, zoom);
        let center = [
            self.target[0] + (anchor[0] - 0.5) * size[0] * (old - new),
            self.target[1] + (anchor[1] - 0.5) * size[1] * (old - new),
        ];
        if self.distinct(size, center, zoom) {
            self.target = center;
            self.target_zoom = zoom;
            self.precision_limited = false;
        } else {
            // A large wheel/pinch gesture approaches the numerical boundary
            // rather than rejecting all useful magnification in that gesture.
            let mut low = 0.;
            let mut high = 1.;
            for _ in 0..52 {
                let amount = (low + high) * 0.5;
                let trial = self.target_zoom + delta * amount;
                let u = self.unit(size, trial);
                let center = [
                    self.target[0] + (anchor[0] - 0.5) * size[0] * (old - u),
                    self.target[1] + (anchor[1] - 0.5) * size[1] * (old - u),
                ];
                if self.distinct(size, center, trial) {
                    low = amount;
                } else {
                    high = amount;
                }
            }
            let trial = self.target_zoom + delta * low;
            let u = self.unit(size, trial);
            self.target = [
                self.target[0] + (anchor[0] - 0.5) * size[0] * (old - u),
                self.target[1] + (anchor[1] - 0.5) * size[1] * (old - u),
            ];
            self.target_zoom = trial;
            self.precision_limited = true;
        }
    }
    /// Drag by a pixel delta while retaining geometry under the pointer.
    pub fn pan(&mut self, delta: [f64; 2], size: [f64; 2]) {
        if !delta.iter().all(|v| v.is_finite()) {
            return;
        }
        let u = self.unit(size, self.target_zoom);
        let center = [self.target[0] - delta[0] * u, self.target[1] - delta[1] * u];
        if self.distinct(size, center, self.target_zoom) {
            self.target = center;
        }
    }
    /// Ease toward the navigation target; return whether the camera changed.
    pub fn advance(&mut self, seconds: f64) -> bool {
        let amount = 1.0 - (-seconds.max(0.0) * 22.0).exp();
        if amount <= 0. {
            return false;
        }
        let changed = self.zoom != self.target_zoom || self.center != self.target;
        if changed {
            let old_unit = self.unit(self.canvas, self.zoom);
            let next = self.zoom + (self.target_zoom - self.zoom) * amount;
            self.zoom = if (next - self.target_zoom).abs() < 1e-10 || next == self.zoom {
                self.target_zoom
            } else {
                next
            };
            let new_unit = self.unit(self.canvas, self.zoom);
            let target_unit = self.unit(self.canvas, self.target_zoom);
            for i in 0..2 {
                self.center[i] += (self.anchor[i] - 0.5) * self.canvas[i] * (old_unit - new_unit);
                let target = self.target[i]
                    + (self.anchor[i] - 0.5) * self.canvas[i] * (target_unit - new_unit);
                let next = self.center[i] + (target - self.center[i]) * amount;
                self.center[i] = if next == self.center[i] { target } else { next };
            }
        }
        changed
    }
    /// Camera rectangle preserving f64 coordinates through rendering and export.
    pub fn precise_viewport(&self, size: [f64; 2]) -> [f64; 4] {
        self.rectangle(size, self.center, self.zoom)
    }
    /// Uniform camera rectangle: updates sampling without recompiling the document.
    pub fn viewport(&self, size: [f64; 2]) -> [f32; 4] {
        self.precise_viewport(size).map(|v| v as f32)
    }
}
