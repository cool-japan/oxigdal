//! Animation and easing utilities for smooth transitions
//!
//! This module provides animation support for the WASM viewer, including:

// Allow unused code in this module as it provides a comprehensive animation API
// that may not be fully utilized initially
#![allow(dead_code)]
//! - Easing functions (linear, quadratic, cubic, quartic, exponential, etc.)
//! - Animation state management
//! - Smooth pan and zoom transitions
//! - Spring physics for natural motion
//! - Animation sequencing and composition
//!
//! # Overview
//!
//! Animations are essential for creating smooth, professional user experiences
//! in interactive map viewers. This module provides tools for:
//!
//! ## Easing Functions
//!
//! Easing functions control the rate of change over time. Available functions:
//! - Linear: constant speed
//! - Quadratic: gradual acceleration/deceleration
//! - Cubic: more pronounced curves
//! - Quartic: even smoother motion
//! - Quintic: very smooth, natural feel
//! - Sine: smooth start and end
//! - Exponential: dramatic acceleration
//! - Circular: arc-based motion
//! - Elastic: spring-like bounce
//! - Back: slight overshoot
//! - Bounce: bouncing effect
//!
//! Each has "in", "out", and "in-out" variants for different effects.
//!
//! ## Animation State
//!
//! The `Animation` struct manages the state of an ongoing animation:
//! - Start and end values
//! - Duration and progress
//! - Easing function
//! - Current value calculation
//!
//! ## Pan and Zoom Animations
//!
//! Specialized animation types for common map operations:
//! - `PanAnimation`: smooth camera panning
//! - `ZoomAnimation`: smooth zoom in/out with focal point
//! - `FitBoundsAnimation`: animate to fit a bounding box
//!
//! ## Spring Physics
//!
//! The `SpringAnimation` provides physics-based motion that feels natural:
//! - Configurable stiffness and damping
//! - Automatic settling detection
//! - Velocity-based motion
//!
//! ## Animation Sequencing
//!
//! Combine multiple animations:
//! - Sequential: play animations one after another
//! - Parallel: play multiple animations simultaneously
//! - Delayed: start animations after a delay
//!
//! # Examples
//!
//! ## Basic Easing
//!
//! ```rust
//! use oxigdal_wasm::{Easing, EasingFunction};
//!
//! let easing = Easing::QuadraticInOut;
//! let t = 0.5; // Halfway through
//! let value = easing.apply(t);
//! ```
//!
//! ## Simple Animation
//!
//! ```rust
//! use oxigdal_wasm::{Animation, Easing};
//!
//! let mut anim = Animation::new(0.0, 100.0, 1000.0, Easing::QuadraticOut);
//!
//! // Update at each frame
//! anim.update(16.67); // 60 FPS
//! let current_value = anim.current_value();
//! ```
//!
//! ## Pan Animation
//!
//! ```rust
//! use oxigdal_wasm::{PanAnimation, Easing};
//!
//! let mut pan = PanAnimation::new(
//!     (0.0, 0.0),    // Start position
//!     (100.0, 50.0), // End position
//!     500.0,         // Duration (ms)
//!     Easing::CubicOut,
//! );
//!
//! while !pan.is_complete() {
//!     pan.update(16.67);
//!     let (x, y) = pan.current_position();
//!     // Update camera position
//! }
//! ```
//!
//! ## Spring Animation
//!
//! ```rust
//! use oxigdal_wasm::SpringAnimation;
//!
//! let mut spring = SpringAnimation::new(0.0, 100.0, 0.5, 0.8);
//! // stiffness: 0.5, damping: 0.8
//!
//! while !spring.is_settled() {
//!     spring.update(16.67);
//!     let value = spring.current_value();
//! }
//! ```
//!
//! # Performance Considerations
//!
//! - Animations use `f64` for precision
//! - Easing functions are pure and fast
//! - No allocations during updates
//! - Can run hundreds of animations simultaneously
//!
//! # Browser Compatibility
//!
//! Works in all modern browsers. For best performance, sync animations
//! with `requestAnimationFrame`.

use crate::WasmError;

/// Easing function types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Easing {
    /// Constant speed (no easing)
    Linear,

    /// Quadratic easing in (accelerating from zero)
    QuadraticIn,
    /// Quadratic easing out (decelerating to zero)
    QuadraticOut,
    /// Quadratic easing in-out (acceleration then deceleration)
    QuadraticInOut,

    /// Cubic easing in
    CubicIn,
    /// Cubic easing out
    CubicOut,
    /// Cubic easing in-out
    CubicInOut,

    /// Quartic easing in
    QuarticIn,
    /// Quartic easing out
    QuarticOut,
    /// Quartic easing in-out
    QuarticInOut,

    /// Quintic easing in
    QuinticIn,
    /// Quintic easing out
    QuinticOut,
    /// Quintic easing in-out
    QuinticInOut,

    /// Sine easing in
    SineIn,
    /// Sine easing out
    SineOut,
    /// Sine easing in-out
    SineInOut,

    /// Exponential easing in
    ExponentialIn,
    /// Exponential easing out
    ExponentialOut,
    /// Exponential easing in-out
    ExponentialInOut,

    /// Circular easing in
    CircularIn,
    /// Circular easing out
    CircularOut,
    /// Circular easing in-out
    CircularInOut,

    /// Elastic easing in (spring effect)
    ElasticIn,
    /// Elastic easing out (spring effect)
    ElasticOut,
    /// Elastic easing in-out (spring effect)
    ElasticInOut,

    /// Back easing in (slight overshoot)
    BackIn,
    /// Back easing out (slight overshoot)
    BackOut,
    /// Back easing in-out (slight overshoot)
    BackInOut,

    /// Bounce easing in (bouncing effect)
    BounceIn,
    /// Bounce easing out (bouncing effect)
    BounceOut,
    /// Bounce easing in-out (bouncing effect)
    BounceInOut,
}

/// Trait for applying easing functions
pub trait EasingFunction {
    /// Apply easing to a normalized time value (0.0 to 1.0)
    fn apply(&self, t: f64) -> f64;
}

impl EasingFunction for Easing {
    fn apply(&self, t: f64) -> f64 {
        let t = t.clamp(0.0, 1.0);

        match self {
            Easing::Linear => t,

            Easing::QuadraticIn => t * t,
            Easing::QuadraticOut => t * (2.0 - t),
            Easing::QuadraticInOut => {
                if t < 0.5 {
                    2.0 * t * t
                } else {
                    -1.0 + (4.0 - 2.0 * t) * t
                }
            }

            Easing::CubicIn => t * t * t,
            Easing::CubicOut => {
                let t = t - 1.0;
                t * t * t + 1.0
            }
            Easing::CubicInOut => {
                if t < 0.5 {
                    4.0 * t * t * t
                } else {
                    let t = 2.0 * t - 2.0;
                    1.0 + t * t * t / 2.0
                }
            }

            Easing::QuarticIn => t * t * t * t,
            Easing::QuarticOut => {
                let t = t - 1.0;
                1.0 - t * t * t * t
            }
            Easing::QuarticInOut => {
                if t < 0.5 {
                    8.0 * t * t * t * t
                } else {
                    let t = 2.0 * t - 2.0;
                    1.0 - t * t * t * t / 2.0
                }
            }

            Easing::QuinticIn => t * t * t * t * t,
            Easing::QuinticOut => {
                let t = t - 1.0;
                t * t * t * t * t + 1.0
            }
            Easing::QuinticInOut => {
                if t < 0.5 {
                    16.0 * t * t * t * t * t
                } else {
                    let t = 2.0 * t - 2.0;
                    1.0 + t * t * t * t * t / 2.0
                }
            }

            Easing::SineIn => 1.0 - (t * std::f64::consts::PI / 2.0).cos(),
            Easing::SineOut => (t * std::f64::consts::PI / 2.0).sin(),
            Easing::SineInOut => -0.5 * ((std::f64::consts::PI * t).cos() - 1.0),

            Easing::ExponentialIn => {
                if t == 0.0 {
                    0.0
                } else {
                    2.0_f64.powf(10.0 * (t - 1.0))
                }
            }
            Easing::ExponentialOut => {
                if t == 1.0 {
                    1.0
                } else {
                    1.0 - 2.0_f64.powf(-10.0 * t)
                }
            }
            Easing::ExponentialInOut => {
                if t == 0.0 || t == 1.0 {
                    t
                } else if t < 0.5 {
                    0.5 * 2.0_f64.powf(20.0 * t - 10.0)
                } else {
                    1.0 - 0.5 * 2.0_f64.powf(-20.0 * t + 10.0)
                }
            }

            Easing::CircularIn => 1.0 - (1.0 - t * t).sqrt(),
            Easing::CircularOut => (2.0 * t - t * t).sqrt(),
            Easing::CircularInOut => {
                if t < 0.5 {
                    0.5 * (1.0 - (1.0 - 4.0 * t * t).sqrt())
                } else {
                    0.5 * ((2.0 * t - 1.0) * (3.0 - 2.0 * t) * 4.0).sqrt() + 0.5
                }
            }

            Easing::ElasticIn => {
                if t == 0.0 || t == 1.0 {
                    t
                } else {
                    let p = 0.3;
                    let s = p / 4.0;
                    let t = t - 1.0;
                    -(2.0_f64.powf(10.0 * t)) * ((t - s) * (2.0 * std::f64::consts::PI) / p).sin()
                }
            }
            Easing::ElasticOut => {
                if t == 0.0 || t == 1.0 {
                    t
                } else {
                    let p = 0.3;
                    let s = p / 4.0;
                    2.0_f64.powf(-10.0 * t) * ((t - s) * (2.0 * std::f64::consts::PI) / p).sin()
                        + 1.0
                }
            }
            Easing::ElasticInOut => {
                if t == 0.0 || t == 1.0 {
                    t
                } else {
                    let p = 0.45;
                    let s = p / 4.0;

                    if t < 0.5 {
                        let t = 2.0 * t - 1.0;
                        -0.5 * 2.0_f64.powf(10.0 * t)
                            * ((t - s) * (2.0 * std::f64::consts::PI) / p).sin()
                    } else {
                        let t = 2.0 * t - 1.0;
                        0.5 * 2.0_f64.powf(-10.0 * t)
                            * ((t - s) * (2.0 * std::f64::consts::PI) / p).sin()
                            + 1.0
                    }
                }
            }

            Easing::BackIn => {
                let c1 = 1.70158;
                let c3 = c1 + 1.0;
                c3 * t * t * t - c1 * t * t
            }
            Easing::BackOut => {
                let c1 = 1.70158;
                let c3 = c1 + 1.0;
                let t = t - 1.0;
                1.0 + c3 * t * t * t + c1 * t * t
            }
            Easing::BackInOut => {
                let c1 = 1.70158;
                let c2 = c1 * 1.525;

                if t < 0.5 {
                    let t = 2.0 * t;
                    (t * t * ((c2 + 1.0) * t - c2)) / 2.0
                } else {
                    let t = 2.0 * t - 2.0;
                    (t * t * ((c2 + 1.0) * t + c2) + 2.0) / 2.0
                }
            }

            Easing::BounceIn => 1.0 - Easing::BounceOut.apply(1.0 - t),
            Easing::BounceOut => {
                let n1 = 7.5625;
                let d1 = 2.75;

                if t < 1.0 / d1 {
                    n1 * t * t
                } else if t < 2.0 / d1 {
                    let t = t - 1.5 / d1;
                    n1 * t * t + 0.75
                } else if t < 2.5 / d1 {
                    let t = t - 2.25 / d1;
                    n1 * t * t + 0.9375
                } else {
                    let t = t - 2.625 / d1;
                    n1 * t * t + 0.984375
                }
            }
            Easing::BounceInOut => {
                if t < 0.5 {
                    0.5 * Easing::BounceIn.apply(2.0 * t)
                } else {
                    0.5 * Easing::BounceOut.apply(2.0 * t - 1.0) + 0.5
                }
            }
        }
    }
}

/// Basic animation state
#[derive(Debug, Clone)]
pub struct Animation {
    start_value: f64,
    end_value: f64,
    duration_ms: f64,
    elapsed_ms: f64,
    easing: Easing,
}

impl Animation {
    /// Create a new animation
    pub fn new(start_value: f64, end_value: f64, duration_ms: f64, easing: Easing) -> Self {
        Self {
            start_value,
            end_value,
            duration_ms,
            elapsed_ms: 0.0,
            easing,
        }
    }

    /// Update animation with time delta
    pub fn update(&mut self, delta_ms: f64) {
        self.elapsed_ms += delta_ms;
        if self.elapsed_ms > self.duration_ms {
            self.elapsed_ms = self.duration_ms;
        }
    }

    /// Get current value
    pub fn current_value(&self) -> f64 {
        let t = if self.duration_ms > 0.0 {
            self.elapsed_ms / self.duration_ms
        } else {
            1.0
        };

        let eased_t = self.easing.apply(t);
        self.start_value + (self.end_value - self.start_value) * eased_t
    }

    /// Check if animation is complete
    pub fn is_complete(&self) -> bool {
        self.elapsed_ms >= self.duration_ms
    }

    /// Get progress (0.0 to 1.0)
    pub fn progress(&self) -> f64 {
        if self.duration_ms > 0.0 {
            (self.elapsed_ms / self.duration_ms).min(1.0)
        } else {
            1.0
        }
    }

    /// Reset animation to beginning
    pub fn reset(&mut self) {
        self.elapsed_ms = 0.0;
    }

    /// Reverse the animation
    pub fn reverse(&mut self) {
        std::mem::swap(&mut self.start_value, &mut self.end_value);
        self.elapsed_ms = 0.0;
    }
}

/// Animation for panning the viewport
#[derive(Debug, Clone)]
pub struct PanAnimation {
    start_pos: (f64, f64),
    end_pos: (f64, f64),
    duration_ms: f64,
    elapsed_ms: f64,
    easing: Easing,
}

impl PanAnimation {
    /// Create a new pan animation
    pub fn new(
        start_pos: (f64, f64),
        end_pos: (f64, f64),
        duration_ms: f64,
        easing: Easing,
    ) -> Self {
        Self {
            start_pos,
            end_pos,
            duration_ms,
            elapsed_ms: 0.0,
            easing,
        }
    }

    /// Update animation
    pub fn update(&mut self, delta_ms: f64) {
        self.elapsed_ms += delta_ms;
        if self.elapsed_ms > self.duration_ms {
            self.elapsed_ms = self.duration_ms;
        }
    }

    /// Get current position
    pub fn current_position(&self) -> (f64, f64) {
        let t = if self.duration_ms > 0.0 {
            self.elapsed_ms / self.duration_ms
        } else {
            1.0
        };

        let eased_t = self.easing.apply(t);
        let x = self.start_pos.0 + (self.end_pos.0 - self.start_pos.0) * eased_t;
        let y = self.start_pos.1 + (self.end_pos.1 - self.start_pos.1) * eased_t;

        (x, y)
    }

    /// Check if complete
    pub fn is_complete(&self) -> bool {
        self.elapsed_ms >= self.duration_ms
    }

    /// Get progress
    pub fn progress(&self) -> f64 {
        if self.duration_ms > 0.0 {
            (self.elapsed_ms / self.duration_ms).min(1.0)
        } else {
            1.0
        }
    }
}

/// Animation for zooming the viewport
#[derive(Debug, Clone)]
pub struct ZoomAnimation {
    start_zoom: f64,
    end_zoom: f64,
    focal_point: (f64, f64),
    duration_ms: f64,
    elapsed_ms: f64,
    easing: Easing,
}

impl ZoomAnimation {
    /// Create a new zoom animation
    pub fn new(
        start_zoom: f64,
        end_zoom: f64,
        focal_point: (f64, f64),
        duration_ms: f64,
        easing: Easing,
    ) -> Self {
        Self {
            start_zoom,
            end_zoom,
            focal_point,
            duration_ms,
            elapsed_ms: 0.0,
            easing,
        }
    }

    /// Update animation
    pub fn update(&mut self, delta_ms: f64) {
        self.elapsed_ms += delta_ms;
        if self.elapsed_ms > self.duration_ms {
            self.elapsed_ms = self.duration_ms;
        }
    }

    /// Get current zoom level
    pub fn current_zoom(&self) -> f64 {
        let t = if self.duration_ms > 0.0 {
            self.elapsed_ms / self.duration_ms
        } else {
            1.0
        };

        let eased_t = self.easing.apply(t);
        self.start_zoom + (self.end_zoom - self.start_zoom) * eased_t
    }

    /// Get focal point
    pub fn focal_point(&self) -> (f64, f64) {
        self.focal_point
    }

    /// Check if complete
    pub fn is_complete(&self) -> bool {
        self.elapsed_ms >= self.duration_ms
    }

    /// Get progress
    pub fn progress(&self) -> f64 {
        if self.duration_ms > 0.0 {
            (self.elapsed_ms / self.duration_ms).min(1.0)
        } else {
            1.0
        }
    }
}

/// Spring animation using physics simulation
#[derive(Debug, Clone)]
pub struct SpringAnimation {
    current: f64,
    target: f64,
    velocity: f64,
    stiffness: f64,
    damping: f64,
    threshold: f64,
}

impl SpringAnimation {
    /// Create a new spring animation
    ///
    /// - `current`: starting value
    /// - `target`: target value
    /// - `stiffness`: spring stiffness (0.0 to 1.0, higher = faster)
    /// - `damping`: damping factor (0.0 to 1.0, higher = less bounce)
    pub fn new(current: f64, target: f64, stiffness: f64, damping: f64) -> Self {
        Self {
            current,
            target,
            velocity: 0.0,
            stiffness: stiffness.clamp(0.0, 1.0),
            damping: damping.clamp(0.0, 1.0),
            threshold: 0.01,
        }
    }

    /// Update spring animation
    pub fn update(&mut self, delta_ms: f64) {
        let delta_s = delta_ms / 1000.0; // Convert to seconds

        // Spring force
        let spring_force = (self.target - self.current) * self.stiffness;

        // Damping force
        let damping_force = -self.velocity * self.damping;

        // Update velocity and position
        self.velocity += (spring_force + damping_force) * delta_s;
        self.current += self.velocity * delta_s;
    }

    /// Get current value
    pub fn current_value(&self) -> f64 {
        self.current
    }

    /// Check if spring has settled
    pub fn is_settled(&self) -> bool {
        (self.current - self.target).abs() < self.threshold && self.velocity.abs() < self.threshold
    }

    /// Set new target
    pub fn set_target(&mut self, target: f64) {
        self.target = target;
    }

    /// Set threshold for settlement detection
    pub fn set_threshold(&mut self, threshold: f64) {
        self.threshold = threshold;
    }
}

/// Animation sequence manager
pub struct AnimationSequence {
    animations: Vec<Box<dyn AnimationTrait>>,
    current_index: usize,
}

/// Trait for animation types
pub trait AnimationTrait {
    /// Update the animation
    fn update(&mut self, delta_ms: f64);

    /// Check if animation is complete
    fn is_complete(&self) -> bool;

    /// Get progress (0.0 to 1.0)
    fn progress(&self) -> f64;
}

impl AnimationTrait for Animation {
    fn update(&mut self, delta_ms: f64) {
        Animation::update(self, delta_ms);
    }

    fn is_complete(&self) -> bool {
        Animation::is_complete(self)
    }

    fn progress(&self) -> f64 {
        Animation::progress(self)
    }
}

impl AnimationTrait for PanAnimation {
    fn update(&mut self, delta_ms: f64) {
        PanAnimation::update(self, delta_ms);
    }

    fn is_complete(&self) -> bool {
        PanAnimation::is_complete(self)
    }

    fn progress(&self) -> f64 {
        PanAnimation::progress(self)
    }
}

impl AnimationTrait for ZoomAnimation {
    fn update(&mut self, delta_ms: f64) {
        ZoomAnimation::update(self, delta_ms);
    }

    fn is_complete(&self) -> bool {
        ZoomAnimation::is_complete(self)
    }

    fn progress(&self) -> f64 {
        ZoomAnimation::progress(self)
    }
}

impl AnimationSequence {
    /// Create a new animation sequence
    pub fn new() -> Self {
        Self {
            animations: Vec::new(),
            current_index: 0,
        }
    }

    /// Add an animation to the sequence
    pub fn add<A: AnimationTrait + 'static>(&mut self, animation: A) {
        self.animations.push(Box::new(animation));
    }

    /// Update the current animation in the sequence
    pub fn update(&mut self, delta_ms: f64) -> Result<(), WasmError> {
        if self.current_index >= self.animations.len() {
            return Ok(());
        }

        let current = &mut self.animations[self.current_index];
        current.update(delta_ms);

        if current.is_complete() {
            self.current_index += 1;
        }

        Ok(())
    }

    /// Check if the entire sequence is complete
    pub fn is_complete(&self) -> bool {
        self.current_index >= self.animations.len()
    }

    /// Get overall progress
    pub fn progress(&self) -> f64 {
        if self.animations.is_empty() {
            return 1.0;
        }

        let completed = self.current_index as f64;
        let current_progress = if self.current_index < self.animations.len() {
            self.animations[self.current_index].progress()
        } else {
            0.0
        };

        (completed + current_progress) / self.animations.len() as f64
    }

    /// Reset the sequence to the beginning
    pub fn reset(&mut self) {
        self.current_index = 0;
    }
}

impl Default for AnimationSequence {
    fn default() -> Self {
        Self::new()
    }
}

/// Parallel animation manager (runs multiple animations simultaneously)
pub struct ParallelAnimation {
    animations: Vec<Box<dyn AnimationTrait>>,
}

impl ParallelAnimation {
    /// Create a new parallel animation manager
    pub fn new() -> Self {
        Self {
            animations: Vec::new(),
        }
    }

    /// Add an animation
    pub fn add<A: AnimationTrait + 'static>(&mut self, animation: A) {
        self.animations.push(Box::new(animation));
    }

    /// Update all animations
    pub fn update(&mut self, delta_ms: f64) {
        for anim in &mut self.animations {
            if !anim.is_complete() {
                anim.update(delta_ms);
            }
        }
    }

    /// Check if all animations are complete
    pub fn is_complete(&self) -> bool {
        self.animations.iter().all(|a| a.is_complete())
    }

    /// Get average progress
    pub fn progress(&self) -> f64 {
        if self.animations.is_empty() {
            return 1.0;
        }

        let total: f64 = self.animations.iter().map(|a| a.progress()).sum();
        total / self.animations.len() as f64
    }
}

impl Default for ParallelAnimation {
    fn default() -> Self {
        Self::new()
    }
}

/// Delayed animation (starts after a delay)
#[derive(Debug)]
pub struct DelayedAnimation<A: AnimationTrait> {
    animation: A,
    delay_ms: f64,
    elapsed_delay_ms: f64,
    started: bool,
}

impl<A: AnimationTrait> DelayedAnimation<A> {
    /// Create a new delayed animation
    pub fn new(animation: A, delay_ms: f64) -> Self {
        Self {
            animation,
            delay_ms,
            elapsed_delay_ms: 0.0,
            started: false,
        }
    }

    /// Update animation (including delay)
    pub fn update(&mut self, delta_ms: f64) {
        if !self.started {
            self.elapsed_delay_ms += delta_ms;
            if self.elapsed_delay_ms >= self.delay_ms {
                self.started = true;
                let overflow = self.elapsed_delay_ms - self.delay_ms;
                if overflow > 0.0 {
                    self.animation.update(overflow);
                }
            }
        } else {
            self.animation.update(delta_ms);
        }
    }

    /// Check if complete
    pub fn is_complete(&self) -> bool {
        self.started && self.animation.is_complete()
    }

    /// Get progress (including delay)
    pub fn progress(&self) -> f64 {
        if !self.started {
            (self.elapsed_delay_ms / self.delay_ms).min(1.0) * 0.5
        } else {
            0.5 + self.animation.progress() * 0.5
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_linear_easing() {
        let easing = Easing::Linear;
        assert_eq!(easing.apply(0.0), 0.0);
        assert_eq!(easing.apply(0.5), 0.5);
        assert_eq!(easing.apply(1.0), 1.0);
    }

    #[test]
    fn test_quadratic_easing() {
        let easing = Easing::QuadraticIn;
        assert_eq!(easing.apply(0.0), 0.0);
        assert!(easing.apply(0.5) < 0.5);
        assert_eq!(easing.apply(1.0), 1.0);
    }

    #[test]
    fn test_animation_basic() {
        let mut anim = Animation::new(0.0, 100.0, 1000.0, Easing::Linear);

        assert_eq!(anim.current_value(), 0.0);
        assert!(!anim.is_complete());

        anim.update(500.0);
        assert!((anim.current_value() - 50.0).abs() < 0.01);

        anim.update(500.0);
        assert_eq!(anim.current_value(), 100.0);
        assert!(anim.is_complete());
    }

    #[test]
    fn test_pan_animation() {
        let mut pan = PanAnimation::new((0.0, 0.0), (100.0, 50.0), 1000.0, Easing::Linear);

        let (x, y) = pan.current_position();
        assert_eq!(x, 0.0);
        assert_eq!(y, 0.0);

        pan.update(500.0);
        let (x, y) = pan.current_position();
        assert!((x - 50.0).abs() < 0.01);
        assert!((y - 25.0).abs() < 0.01);
    }

    #[test]
    fn test_spring_animation() {
        // Use higher stiffness and damping for predictable settling
        let mut spring = SpringAnimation::new(0.0, 100.0, 0.8, 0.98);
        spring.set_threshold(1.0); // Larger threshold for easier settling

        // Run spring for enough iterations to settle
        for _ in 0..1000 {
            spring.update(16.67);
            if spring.is_settled() {
                break;
            }
        }

        // Spring should settle close to target (within 5% tolerance)
        assert!((spring.current_value() - 100.0).abs() < 5.0);
    }

    #[test]
    fn test_animation_reverse() {
        let mut anim = Animation::new(0.0, 100.0, 1000.0, Easing::Linear);
        anim.update(500.0);

        anim.reverse();
        assert_eq!(anim.current_value(), 100.0);

        anim.update(500.0);
        assert_eq!(anim.current_value(), 50.0);
    }
}
