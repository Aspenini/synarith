/// Smooth, cursor-anchored navigation in document coordinates.
#[derive(Clone, Debug)]
pub struct Camera {
    home: [f64; 4],
    center: [f64; 2],
    target: [f64; 2],
    zoom: f64,
    target_zoom: f64,
    /// Whether further navigation would collapse adjacent f32 samples.
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
        (self.home[2] / size[0].max(1.0)).max(self.home[3] / size[1].max(1.0)) / zoom.exp()
    }
    fn rectangle(&self, size: [f64; 2], center: [f64; 2], zoom: f64) -> [f32; 4] {
        let u = self.unit(size, zoom);
        [
            (center[0] - size[0] * u * 0.5) as f32,
            (center[1] - size[1] * u * 0.5) as f32,
            (size[0] * u) as f32,
            (size[1] * u) as f32,
        ]
    }
    fn distinct(&self, size: [f64; 2], center: [f64; 2], zoom: f64) -> bool {
        let v = self.rectangle(size, center, zoom);
        let u = self.unit(size, zoom) as f32;
        v.iter().all(|v| v.is_finite())
            && u > 0.0
            && v[2] > 0.0
            && v[3] > 0.0
            && center[0] as f32 + u != center[0] as f32
            && center[1] as f32 + u != center[1] as f32
            && v[0] + u != v[0]
            && v[1] + u != v[1]
    }
    /// Change logarithmic zoom while retaining the world point under a normalized cursor.
    pub fn zoom_at(&mut self, delta: f64, anchor: [f64; 2], size: [f64; 2]) {
        if !delta.is_finite() {
            return;
        }
        let zoom = (self.target_zoom + delta).clamp(-80.0, 80.0);
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
            self.precision_limited = true;
        }
    }
    /// Drag by a pixel delta while retaining geometry under the pointer.
    pub fn pan(&mut self, delta: [f64; 2], size: [f64; 2]) {
        let u = self.unit(size, self.target_zoom);
        let center = [self.target[0] - delta[0] * u, self.target[1] - delta[1] * u];
        if self.distinct(size, center, self.target_zoom) {
            self.target = center;
        }
    }
    /// Ease toward the navigation target; return whether the camera changed.
    pub fn advance(&mut self, seconds: f64) -> bool {
        let amount = 1.0 - (-seconds.max(0.0) * 22.0).exp();
        let changed = (self.zoom - self.target_zoom).abs() > 1e-8
            || self
                .center
                .iter()
                .zip(self.target)
                .any(|(a, b)| (a - b).abs() > self.home[2].max(self.home[3]) * 1e-10);
        if changed {
            self.zoom += (self.target_zoom - self.zoom) * amount;
            for i in 0..2 {
                self.center[i] += (self.target[i] - self.center[i]) * amount;
            }
        }
        changed
    }
    /// Uniform camera rectangle: updates sampling without recompiling the document.
    pub fn viewport(&self, size: [f64; 2]) -> [f32; 4] {
        self.rectangle(size, self.center, self.zoom)
    }
}
