use crate::transcoder::event_pixel_tree::Mode::{Continuous, FramePerfect};
use crate::{Coord, Event, TimeMode, UDshift, D_EMPTY, D_MAX, D_SHIFT, D_ZERO_INTEGRATION};
use smallvec::{smallvec, SmallVec};
use std::cmp::min;

/// Decimation value; a pixel's sensitivity.
pub type D = u8;

// type Integration = f32;

/// Number of ticks elapsed since a given pixel last fired an [`pixel::Event`]
pub type DeltaT = u32;

/// Measure of an amount of light intensity. Is f32 so that we can use `fast_math::log2_raw`
pub type Intensity32 = f32;

/// Pixel x- or y- coordinate address in the ADΔER model
// pub type PixelAddress = u16;
#[derive(Clone, Copy, PartialEq)]
pub enum Mode {
    FramePerfect,
    Continuous,
}

#[repr(packed)]
#[derive(Copy, Clone, Debug)]
pub(crate) struct Event64 {
    pub coord: Coord,
    pub d: D,
    pub delta_t: f64,
}

impl From<Event64> for Event {
    fn from(event_64: Event64) -> Self {
        Event {
            coord: event_64.coord,
            d: event_64.d,
            delta_t: event_64.delta_t as DeltaT,
        }
    }
}

#[repr(packed)]
#[derive(Copy, Clone, Debug)]
struct PixelState {
    d: D,
    integration: Intensity32,
    delta_t: f64,
}

#[repr(packed)]
#[derive(Clone, Copy, Debug)]
pub struct PixelNode {
    /// Will have the smaller D value
    alt: Option<()>,

    state: PixelState,
    pub(crate) best_event: Option<Event64>,
}

// Each PixelNode is ~20 bytes. Each PixelArena is at least 20 + (6*20) 140 bytes, but takes at
// least 144 bytes of space, I think?
pub struct PixelArena {
    pub coord: Coord,
    time_mode: TimeMode,
    last_fired_t: f64,
    length: usize,
    pub base_val: u8,
    pub need_to_pop_top: bool,
    pub arena: SmallVec<[PixelNode; 6]>,
}

impl PixelArena {
    pub(crate) fn new(start_intensity: Intensity32, coord: Coord) -> PixelArena {
        // let mut arena = Vec::with_capacity(5);
        let mut arena = smallvec![];
        arena.push(PixelNode::new(start_intensity));
        PixelArena {
            coord,
            length: 1,
            time_mode: TimeMode::default(),
            last_fired_t: 0.0,
            base_val: 0,
            need_to_pop_top: false,
            arena,
        }
    }

    pub(crate) fn time_mode(&mut self, time_mode: Option<TimeMode>) {
        if let Some(time_mode) = time_mode {
            self.time_mode = time_mode;
        }
    }

    /// If the integration is 0, we need to forcefully fire an event where d=254
    fn get_zero_event(
        &mut self,
        idx: usize,
        next_intensity: Option<Intensity32>,
        mode: Mode,
        ref_time: DeltaT,
    ) -> Event {
        let mut node = &mut self.arena[idx];
        let mut ret_event = Event64 {
            coord: self.coord,
            d: D_ZERO_INTEGRATION, // 254_u8
            delta_t: node.state.delta_t,
        };
        node.state.delta_t = 0.0;
        match next_intensity {
            None => {}
            Some(intensity) => node.state.d = get_d_from_intensity(intensity),
        }
        debug_assert!(node.alt.is_none());

        let event: Event = self.delta_t_to_absolute_t(&mut ret_event, mode, ref_time);

        event
    }

    fn delta_t_to_absolute_t(
        &mut self,
        event: &mut Event64,
        mode: Mode,
        ref_time: DeltaT,
    ) -> Event {
        // Handle AbsoluteT mode
        if self.time_mode == TimeMode::AbsoluteT {
            event.delta_t += self.last_fired_t;
            self.last_fired_t = event.delta_t;
            if mode == FramePerfect {
                self.last_fired_t = if self.last_fired_t as DeltaT % ref_time == 0 {
                    (self.last_fired_t as DeltaT).into()
                } else {
                    (((self.last_fired_t as DeltaT / ref_time) + 1) * ref_time).into()
                };
            }
        }
        Event {
            coord: self.coord,
            d: event.d,
            delta_t: event.delta_t as DeltaT,
        }
    }

    /// Pop just the topmost event. Should be called only when dtm is reached for main node
    pub fn pop_top_event(
        &mut self,
        next_intensity: Intensity32,
        mode: Mode,
        ref_time: DeltaT,
    ) -> Event {
        self.need_to_pop_top = false;
        let mut root = &mut self.arena[0];
        match root.best_event {
            None => {
                if root.state.integration == 0.0 && root.state.delta_t > 0.0 {
                    self.get_zero_event(0, Some(next_intensity), mode, ref_time)
                } else {
                    // We can reach here under frame-perfect integration when approaching dtm. The new
                    // node might not have the right D set.
                    // TODO: cover with a unit test
                    root.best_event = Some(Event64 {
                        coord: self.coord,
                        d: unsafe {
                            // This should be safe, because we know that the integration will
                            // not exceed 2^D_MAX
                            fast_math::log2_raw(root.state.integration).to_int_unchecked::<D>()
                        },
                        delta_t: root.state.delta_t,
                    });

                    if self.arena.len() > 1 {
                        self.arena[1] = PixelNode::new(next_intensity);
                        self.length = 2;
                    } else {
                        self.arena.push(PixelNode::new(next_intensity));
                        self.length += 1;
                    }

                    self.pop_top_event(next_intensity, mode, ref_time)
                    // panic!("No best event! TODO: handle it")
                }
            }
            Some(mut event) => {
                assert!(self.length > 1);
                for i in 0..self.length - 1 {
                    self.arena[i] = self.arena[i + 1];
                }
                self.length -= 1;
                debug_assert!(self.arena[self.length - 1].alt.is_none());

                self.delta_t_to_absolute_t(&mut event, mode, ref_time)
            }
        }
    }

    /// Recursively pop all the alt events
    pub fn pop_best_events(&mut self, buffer: &mut Vec<Event>, mode: Mode, ref_time: DeltaT) {
        // let mut events = Vec::new();

        for node_idx in 0..self.length {
            match self.arena[node_idx].best_event {
                None => {
                    if self.arena[node_idx].state.delta_t > 0.0
                        && self.arena[node_idx].state.integration == 0.0
                    {
                        buffer.push(self.get_zero_event(node_idx, None, mode, ref_time));
                    }
                }
                Some(mut event) => {
                    assert_ne!(node_idx, self.length - 1);
                    let event = self.delta_t_to_absolute_t(&mut event, mode, ref_time);
                    buffer.push(event);
                }
            }
        }

        // Move the last node to the front
        self.arena.swap(0, self.length - 1);
        assert!(self.arena[0].alt.is_none());
        self.length = 1;

        // match next_intensity {
        //     None => {}
        //     // TODO: match on mode instead. This is disjoint.
        //     Some(intensity) => {
        // self.arena[0].state.d = get_d_from_intensity(intensity);
        // self.arena[0].state.integration = 0.0;
        // self.arena[0].state.delta_t = 0.0;
        //     }
        // };
        self.need_to_pop_top = false;
    }

    pub fn set_d_for_continuous(&mut self, next_intensity: Intensity32) -> Option<Event> {
        let head = &mut self.arena[0];
        let next_d = get_d_from_intensity(next_intensity);
        let ret = if next_d < head.state.d && head.state.delta_t > 0.0 {
            let ret = Some(Event {
                coord: self.coord,
                d: D_EMPTY,
                delta_t: (head.state.delta_t) as DeltaT,
            });
            head.state.delta_t = 0.0;
            head.state.integration = 0.0;
            ret
        } else {
            None
        };
        head.state.d = next_d;
        ret
    }

    /// Integrates the intensity. Returns bool indicating whether or not the topmost event MUST be popped
    /// or else risk losing accuracy. Should only return true when `d=D_MAX`, which should be
    /// extremely rare, or when `delta_t_max` is hit
    pub fn integrate(
        &mut self,
        mut intensity: Intensity32,
        mut time: f64,
        mode: Mode,
        dtm: DeltaT,
        ref_time: DeltaT,
    ) {
        let tail = &mut self.arena[self.length - 1];
        if tail.state.delta_t == 0.0 && tail.state.integration == 0.0 {
            tail.state.d = get_d_from_intensity(intensity);
        }

        let mut idx = 0;
        loop {
            let filled = match self.integrate_main(idx, intensity, time, mode) {
                None => false,
                Some((next_intensity, next_time)) => {
                    // self.arena.drain(idx + 1..);
                    if self.arena.len() > idx + 1 {
                        self.arena[idx + 1] = PixelNode::new(intensity);
                    } else {
                        self.arena.push(PixelNode::new(intensity));
                    }
                    self.length = idx + 2;
                    self.arena[idx].alt = Some(());
                    intensity = next_intensity;
                    time = next_time;
                    true
                }
            };

            idx += 1;

            if filled {
                match mode {
                    FramePerfect => break,

                    // If continuous, we need to integrate the remaining intensity for the current
                    // node and the branching nodes
                    Continuous => {
                        if time > ref_time as f64 {
                            self.arena[idx].state.d = get_d_from_intensity(intensity);
                        }
                    }
                }
            }

            if idx >= self.length {
                break;
            }
        }
        debug_assert!(self.length <= self.arena.len());
        assert!(self.length > 0);

        self.need_to_pop_top = self.arena[0].state.d == D_MAX
            || unsafe { self.arena[0].state.delta_t.to_int_unchecked::<DeltaT>() } >= dtm;
    }

    /// Integrate an intensity for a given node. Returns `Some()` if the node fires an event, so
    /// that the newly-created branch's node only gets integrated with the remaining intensity.
    #[allow(clippy::similar_names)]
    pub fn integrate_main(
        &mut self,
        index: usize,
        intensity: Intensity32,
        time: f64,
        mode: Mode,
    ) -> Option<(Intensity32, f64)> {
        let node = &mut self.arena[index];
        if node.state.integration + intensity >= D_SHIFT[node.state.d as usize] as f32 {
            // If the new intensity is much bigger, then we need to increase D accordingly, first
            let new_d = get_d_from_intensity(node.state.integration + intensity);
            node.state.d = new_d;

            let prop = (D_SHIFT[node.state.d as usize] as f64 - node.state.integration as f64)
                / intensity as f64;
            assert!(prop > 0.0);
            node.best_event = Some(Event64 {
                coord: self.coord,
                d: node.state.d,
                delta_t: node.state.delta_t + time * prop,
            });

            // Increase d to prepare for the next integration of this pixel
            if node.state.d < D_MAX {
                node.state.integration += intensity;
                node.state.delta_t += time;

                // TODO: this is slow and dumb
                loop {
                    node.state.d += 1;
                    if D_SHIFT[node.state.d as usize] > node.state.integration as UDshift {
                        break;
                    }
                }
            } else {
                // dbg!(node.state.integration);
            }

            if intensity as f64 - (intensity as f64 * prop) >= 0.0 {
                // For a framed source, we need to return 0,0 for intensity,time.
                // This lets us preserve the spatially-coherent intensities, especially for color
                // transcode.

                return Some(match mode {
                    FramePerfect => (0.0, 0.0),
                    Continuous => (intensity - (intensity * prop as f32), time - (time * prop)),
                });
            }
            Some((0.0, 0.0))
        } else {
            node.state.integration += intensity;
            node.state.delta_t += time;
            None
        }
    }
}

fn get_d_from_intensity(intensity: Intensity32) -> D {
    min(
        {
            if intensity > 0.0 {
                unsafe { fast_math::log2_raw(intensity).to_int_unchecked::<D>() }
            } else {
                0
            }
        },
        D_MAX,
    )
}

impl PixelNode {
    pub fn new(start_intensity: Intensity32) -> PixelNode {
        let start_d = get_d_from_intensity(start_intensity);
        assert!(start_d <= D_MAX);
        PixelNode {
            alt: None,
            state: PixelState {
                d: start_d,
                integration: 0.0,
                delta_t: 0.0,
            },
            best_event: None,
        }
    }
    // pub fn new2(start_intensity: Intensity32) -> PixelNode {
    //     let start_d = min(get_d_from_intensity(start_intensity) + 1, D_MAX);
    //     assert!(start_d <= D_MAX);
    //     PixelNode {
    //         alt: None,
    //         state: PixelState {
    //             d: start_d,
    //             integration: 0.0,
    //             delta_t: 0.0,
    //         },
    //         best_event: None,
    //     }
    // }

    pub fn set_d(&mut self, d: D) {
        self.state.d = d;
    }
}

#[cfg(test)]
mod tests {
    use float_cmp::approx_eq;
    // Note this useful idiom: importing names from outer (for mod tests) scope.
    use super::*;

    fn make_tree() -> PixelArena {
        let dtm = 10_000;
        let mut tree = PixelArena::new(
            100.0,
            Coord {
                x: 0,
                y: 0,
                c: None,
            },
        );

        assert_eq!(tree.arena[0].state.d, 6);
        tree.integrate(100.0, 20.0, Continuous, dtm, 20);
        assert!(tree.arena[0].best_event.is_some());
        let node = &tree.arena[0];
        match node.best_event {
            None => {
                panic!()
            }
            Some(event) => {
                let event: Event = event.into();
                assert_eq!(event.d, 6);

                // Refer to https://github.com/rust-lang/rust/issues/82523
                let tmp = event.delta_t;
                assert_eq!(tmp, 12);
            }
        }
        assert_eq!(tree.arena[0].state.d, 7);
        assert!(f32_slack(tree.arena[0].state.integration, 100.0));
        assert!(f64_slack(tree.arena[0].state.delta_t, 20.0));
        assert!(tree.arena[0].alt.is_some());

        let node = &tree.arena[1];
        assert!(node.best_event.is_none());
        assert_eq!(node.state.d, 6);
        let tmp = node.state.integration;
        assert_eq!(tmp, 36.0);
        assert!(approx_eq!(f64, tree.arena[1].state.delta_t, 7.2, ulps = 2));

        tree.integrate(100.0, 20.0, Continuous, dtm, 20);
        assert_eq!(tree.arena[0].best_event.unwrap().d, 7);
        // Since we're casting, the delta t gets rounded down
        let tmp = tree.arena[0].best_event.unwrap().delta_t;
        assert!(approx_eq!(f64, tmp, 25.6, ulps = 1));
        assert_eq!(tree.arena[0].state.d, 8);
        assert!(f32_slack(tree.arena[0].state.integration, 200.0));
        assert!(f64_slack(tree.arena[0].state.delta_t, 40.0));
        assert!(tree.arena[0].alt.is_some());
        assert_eq!(tree.arena[1].state.d, 7);
        assert!(f32_slack(tree.arena[1].state.integration, 72.0));
        assert!(approx_eq!(f64, tree.arena[1].state.delta_t, 14.4, ulps = 1));
        assert_eq!(tree.arena[1].best_event.unwrap().d, 6);
        let tmp = tree.arena[1].best_event.unwrap().delta_t;
        assert!(approx_eq!(f64, tmp, 12.8, ulps = 2));
        assert!(tree.arena[1].alt.is_some());
        let alt_alt = &tree.arena[2];
        assert_eq!(alt_alt.state.d, 6);
        assert!(alt_alt.best_event.is_none());
        assert!(alt_alt.alt.is_none());
        assert!(f32_slack(alt_alt.state.integration, 8.0));
        assert!(approx_eq!(
            f64,
            alt_alt.state.delta_t,
            1.6,
            epsilon = 0.2e-14
        ));

        // tree at this point
        // --node states are d, integration, delta_t
        // "best events" are (d, delta_t)
        //  ---------------------------------------------------------------------------8, 200, 40
        //                                        \
        //                                    (7,25)-----------------------------------7, 72, 14.4
        //                                                                  \
        //                                                             (6,12)----------6, 8, 1.6
        tree
    }

    fn make_tree2() -> PixelArena {
        let dtm = 10_000;
        let mut tree = make_tree();
        tree.integrate(30.0, 34.0, Continuous, dtm, 34);

        {
            let root = &tree.arena[0];
            // Main node still not filled
            assert_eq!(root.state.d, 8);
            assert!(f32_slack(root.state.integration, 230.0));
            assert!(f64_slack(root.state.delta_t, 74.0));

            let alt = &tree.arena[1];
            assert_eq!(alt.state.d, 7);
            assert!(f32_slack(alt.state.integration, 102.0));
            assert!(f64_slack(alt.state.delta_t, 48.4));

            let alt = &tree.arena[2];
            assert_eq!(alt.state.d, 6);
            assert!(f32_slack(alt.state.integration, 38.0));
            assert!(f64_slack(alt.state.delta_t, 35.6));
        }

        //  ------------------------------------------------------------8, 230, 74
        //                  \
        //              (7,25)------------------------------------------7, 102, 48.4
        //                                         \
        //                                    (6,12)--------------------6, 38, 35.6

        tree.integrate(26.0, 34.0, Continuous, dtm, 34);
        // Main node just filled
        assert_eq!(tree.arena[0].state.d, 9);
        assert!(f32_slack(tree.arena[0].state.integration, 256.0));
        assert!(f64_slack(tree.arena[0].state.delta_t, 108.0));

        assert_eq!(tree.arena[0].best_event.unwrap().d, 8);
        let tmp = tree.arena[0].best_event.unwrap().delta_t;
        assert_eq!(tmp, 108.0);

        let alt = &tree.arena[1];
        assert_eq!(alt.state.d, 4);
        assert!(f32_slack(alt.state.integration, 0.0));
        assert!(f64_slack(alt.state.delta_t, 0.0));
        assert!(alt.best_event.is_none());
        assert!(alt.alt.is_none());

        //  ---------------------------9, 256, 108
        //                  \
        //              (8,108)--------4, 0, 0
        tree
    }

    #[test]
    fn test_make_tree() {
        make_tree();
    }

    #[test]
    fn test_make_tree2() {
        make_tree2();
    }

    #[test]
    fn test_pop_best_states() {
        let mut tree = make_tree();
        let mut events = Vec::new();
        tree.pop_best_events(&mut events, Continuous, 20);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].d, 7);
        let tmp = events[0].delta_t;
        assert_eq!(tmp, 25);
        assert_eq!(events[1].d, 6);
        let tmp = events[1].delta_t;
        assert_eq!(tmp, 12);
        assert_eq!(tree.arena[0].state.d, 6);
        assert!(f32_slack(tree.arena[0].state.integration, 8.0));
        assert!(approx_eq!(
            f64,
            tree.arena[0].state.delta_t,
            1.6,
            epsilon = 0.2e-14
        ));
    }

    #[test]
    fn test_pop_best_states2() {
        let mut tree = make_tree2();
        let mut events = Vec::new();
        tree.pop_best_events(&mut events, Continuous, 34);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].d, 8);
        let tmp = events[0].delta_t;
        assert_eq!(tmp, 108);
        assert_eq!(tree.arena[0].state.d, 4);
        assert!(f32_slack(tree.arena[0].state.integration, 0.0));
        assert!(f64_slack(tree.arena[0].state.delta_t, 0.0));
    }

    #[test]
    fn test_d_max() {
        // 1048576
        let dtm = 100_000_000;
        let mut tree = PixelArena::new(
            (1u128 << 126u128) as f32,
            Coord {
                x: 0,
                y: 0,
                c: None,
            },
        );
        tree.integrate(
            (1u128 << 126u128) as f32,
            100_000.0,
            Continuous,
            dtm,
            100_000,
        );
        assert!(tree.need_to_pop_top);
        let mut events = Vec::new();
        tree.pop_best_events(&mut events, Continuous, 100_000);
        assert!(!tree.need_to_pop_top);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].d, 126);
        let tmp = events[0].delta_t;
        assert_eq!(tmp, 100_000);
        assert!(f32_slack(tree.arena[0].state.integration, 0.0));
    }

    #[test]
    fn test_dtm() {
        let dtm = 240_000;
        let mut tree = PixelArena::new(
            245.0,
            Coord {
                x: 0,
                y: 0,
                c: None,
            },
        );
        for _ in 0..47 {
            tree.integrate(245.0, 5_000.0, FramePerfect, dtm, 5_000);
        }
        tree.integrate(245.0, 5_000.0, FramePerfect, dtm, 5_000);
        assert!(tree.need_to_pop_top);
        let _ = tree.pop_top_event(245.0, FramePerfect, 5_000);
        assert!(!tree.need_to_pop_top);
        let tmp = tree.arena[0].state.delta_t;
        assert_eq!(tmp, 70_000.0)
    }

    #[test]
    fn test_big_integration() {
        let dtm = 1_000_000;
        let mut tree = PixelArena::new(
            146.0,
            Coord {
                x: 0,
                y: 0,
                c: None,
            },
        );
        tree.integrate(146.0, 2_000.0, Continuous, dtm, 2_000);
        tree.integrate(2_790.863, 38231.0, Continuous, dtm, 38231);

        let head = tree.arena[0];
        let integ = head.state.integration;
        let dt = head.state.delta_t;
        let d = head.state.d;
        assert_eq!(integ, 2_790.863 + 146.0);
        assert_eq!(dt, 38231.0 + 2_000.0);
        assert_eq!(head.best_event.unwrap().d, d - 1);
    }

    #[test]
    fn test_big_integration2() {
        let dtm = 10_000_000;
        let mut tree = PixelArena::new(
            255.0,
            Coord {
                x: 0,
                y: 0,
                c: None,
            },
        );
        loop {
            tree.integrate(255.0, 2_000.0, Continuous, dtm, 2_000);
            if tree.need_to_pop_top {
                break;
            }
        }

        let head = tree.arena[0];
        let d = head.state.d;
        let integ = head.state.integration;
        let dt = head.state.delta_t;

        assert_eq!(integ, 1.275e6);
        assert_eq!(dt, dtm as f64);
        assert_eq!(head.best_event.unwrap().d, d - 1);
    }

    fn f32_slack(num0: f32, num1: f32) -> bool {
        let slack = f32::EPSILON;
        if num1 - slack <= num0 && num1 + slack >= num0 {
            return true;
        }
        false
    }
    fn f64_slack(num0: f64, num1: f64) -> bool {
        let slack = f64::EPSILON;
        if num1 - slack <= num0 && num1 + slack >= num0 {
            return true;
        }
        false
    }

    // Example used in the MMSys '23 paper
    #[test]
    fn test_paper_example() {
        let dtm = 10_000;
        let mut tree = PixelArena::new(
            101.0,
            Coord {
                x: 0,
                y: 0,
                c: None,
            },
        );

        assert_eq!(tree.arena[0].state.d, 6);
        tree.integrate(101.0, 20.0, Continuous, dtm, 20);
        assert!(tree.arena[0].best_event.is_some());

        tree.integrate(40.0, 30.0, Continuous, dtm, 30);
        let event = tree.arena[0].best_event.unwrap();
        assert_eq!(event.d, 7);
        let child = tree.arena[1];
        assert!(f64_slack(child.state.delta_t, 9.75));
    }

    #[test]
    fn test_absolute_mode_1() {
        let dtm = 10_000;
        let mut tree = PixelArena::new(
            101.0,
            Coord {
                x: 0,
                y: 0,
                c: None,
            },
        );
        tree.time_mode(Some(TimeMode::AbsoluteT));

        assert_eq!(tree.arena[0].state.d, 6);
        tree.integrate(101.0, 20.0, Continuous, dtm, 20);
        assert!(tree.arena[0].best_event.is_some());

        tree.integrate(40.0, 30.0, Continuous, dtm, 30);
        tree.integrate(140.0, 30.0, Continuous, dtm, 30);
        tree.integrate(103.0, 30.0, Continuous, dtm, 30);
        let mut events = Vec::new();
        tree.pop_best_events(&mut events, Continuous, 30);
        let dt = events[0].delta_t;
        assert_eq!(events[0].d, 8);
        assert_eq!(dt, 74);
        let dt = events[1].delta_t;
        assert_eq!(events[1].d, 7);
        assert_eq!(dt, 110);
    }
}
