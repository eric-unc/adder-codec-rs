use crate::codec::compressed::blocks::block::BlockEvents;
use crate::codec::compressed::blocks::{
    DResidual, DeltaTResidual, TResidual, BLOCK_SIZE_AREA, D_ENCODE_NO_EVENT,
};
use crate::Mode::FramePerfect;
use crate::{AbsoluteT, DeltaT, EventCoordless, Mode, D};

pub static D_RESIDUALS_EMPTY: [DResidual; BLOCK_SIZE_AREA] = [D_ENCODE_NO_EVENT; BLOCK_SIZE_AREA];

/// Keeps track of the actual and predicted (reconstructed) times of past events, and gets the next
/// prediction residual
pub struct PredictionModel {
    /// Holds the true last t
    pub t_memory: [AbsoluteT; BLOCK_SIZE_AREA],

    /// Holds (reconstructed) delta_t values, regardless of time mode
    pub event_memory: [EventCoordless; BLOCK_SIZE_AREA],

    /// Holds the reconstructed last t
    pub t_recon: [AbsoluteT; BLOCK_SIZE_AREA],
    // TODO: Make the above three private
    /// The encoded d_residuals. Stored here so that we can recycle the memory.
    d_residuals: [DResidual; BLOCK_SIZE_AREA],

    dt_pred_residuals: [DeltaTResidual; BLOCK_SIZE_AREA],

    /// The residuals for the events' delta_t predictions. This is what actually gets arithmetic encoded.
    dt_pred_residuals_i16: [i16; BLOCK_SIZE_AREA],

    pub time_modulation_mode: Mode,
}

impl PredictionModel {
    pub fn new(time_modulation_mode: Mode) -> Self {
        PredictionModel {
            t_memory: [0; BLOCK_SIZE_AREA],
            event_memory: [Default::default(); BLOCK_SIZE_AREA],
            t_recon: [0; BLOCK_SIZE_AREA],
            d_residuals: D_RESIDUALS_EMPTY,
            dt_pred_residuals: [0; BLOCK_SIZE_AREA],
            dt_pred_residuals_i16: [0; BLOCK_SIZE_AREA],
            time_modulation_mode,
        }
    }

    pub fn override_memory(
        &mut self,
        event_memory: [EventCoordless; BLOCK_SIZE_AREA],
        t_recon: [AbsoluteT; BLOCK_SIZE_AREA],
    ) {
        self.event_memory = event_memory;
        self.t_recon = t_recon;
    }

    fn reset_memory(&mut self) {
        self.t_memory = [0; BLOCK_SIZE_AREA];
        self.event_memory = [Default::default(); BLOCK_SIZE_AREA];
        self.t_recon = [0; BLOCK_SIZE_AREA];
    }

    fn reset_residuals(&mut self) {
        // self.t_memory = [0; BLOCK_SIZE_AREA];
        // self.event_memory = [Default::default(); BLOCK_SIZE_AREA],
        self.d_residuals = D_RESIDUALS_EMPTY;
        self.dt_pred_residuals = [0; BLOCK_SIZE_AREA];
        self.dt_pred_residuals_i16 = [0; BLOCK_SIZE_AREA];
    }

    pub(crate) fn forward_intra_prediction(
        &mut self,
        mut sparam: u8,
        dt_ref: DeltaT,
        dtm: DeltaT,
        events: &BlockEvents,
    ) -> (
        AbsoluteT,
        D,
        &[DResidual; BLOCK_SIZE_AREA],
        &[i16; BLOCK_SIZE_AREA],
        u8,
    ) {
        self.reset_residuals();
        self.reset_memory();
        let mut init = false;
        let mut start = EventCoordless { d: 0, delta_t: 0 };

        let mut max_t_resid = 0;

        for (idx, event_opt) in events.iter().enumerate() {
            if let Some(prev) = event_opt {
                // If this is the first event encountered, then encode it directly
                if !init {
                    init = true;
                    // self.d_residuals[idx] = prev.d as DResidual;
                    // self.dt_pred_residuals[idx] = prev.t() as DeltaTResidual;

                    // self.event_memory[idx] = *prev;
                    // self.event_memory[idx].delta_t -= self.t_memory[idx];
                    self.t_memory[idx] = prev.t();
                    if self.time_modulation_mode == FramePerfect && self.t_memory[idx] % dt_ref != 0
                    {
                        self.t_memory[idx] = ((self.t_memory[idx] / dt_ref) + 1) * dt_ref;
                    }

                    self.t_recon[idx] = self.t_memory[idx];

                    // // convert absolute t to delta_t
                    // self.event_memory[idx].delta_t -= prev.delta_t;

                    start = *prev;
                }

                // Get the prediction residual for the next event and store it
                for (next_idx, next_event_opt) in events.iter().skip(idx + 1).enumerate() {
                    if let Some(next) = next_event_opt {
                        let d_resid = next.d as DResidual - start.d as DResidual;
                        let t_resid =
                            next.delta_t as DeltaTResidual - start.delta_t as DeltaTResidual;

                        self.d_residuals[next_idx + idx + 1] = d_resid;
                        self.dt_pred_residuals[next_idx + idx + 1] = t_resid;

                        // self.event_memory[next_idx + idx + 1] = *next;
                        // self.event_memory[next_idx + idx + 1].delta_t -=
                        //     self.t_memory[next_idx + idx + 1];

                        self.t_memory[next_idx + idx + 1] = next.t();
                        if self.time_modulation_mode == FramePerfect
                            && self.t_memory[next_idx + idx + 1] % dt_ref != 0
                        {
                            self.t_memory[next_idx + idx + 1] =
                                ((self.t_memory[next_idx + idx + 1] / dt_ref) + 1) * dt_ref;
                        }
                        self.t_recon[next_idx + idx + 1] = self.t_memory[next_idx + idx + 1];

                        // convert absolute t to delta_t
                        // if self.event_memory[next_idx + idx + 1].delta_t > dtm {
                        //     self.event_memory[next_idx + idx + 1].delta_t -= start.delta_t;
                        // }

                        if t_resid.abs() > max_t_resid {
                            max_t_resid = t_resid.abs();
                            if max_t_resid > dtm as i64 {
                                eprintln!(
                                    "max_t_resid: {}, next_dt: {}, start_dt: {}, ",
                                    max_t_resid, next.delta_t, start.delta_t
                                );
                            }
                            // assert!(max_t_resid <= dtm as i64);
                            // assert!(max_t_resid < 100000000);
                        }
                        break;
                    }
                }
            }
        }

        // if max_t_resid is greater than 2^15, then we need to increase the sparam
        let num_places = max_t_resid.leading_zeros();
        if num_places + (sparam as u32) < 49 && max_t_resid > 0 {
            sparam = (49 - num_places) as u8;
        }

        // Quantize the T residuals
        for (t_resid, t_resid_i16) in self
            .dt_pred_residuals
            .iter()
            .zip(self.dt_pred_residuals_i16.iter_mut())
        {
            assert!(*t_resid >> sparam <= i16::MAX as i64);
            *t_resid_i16 = (*t_resid >> sparam) as i16;
        }

        (
            start.delta_t,
            start.d,
            &self.d_residuals,
            &self.dt_pred_residuals_i16,
            sparam,
        )
    }

    /// Get a block of inter-prediction residuals. `t_memory` should hold the previous absolute t
    /// values for each pixel in the block. If the previous block was also inter-coded, then this
    /// memory should be the _reconstructed_ t values after compression (to prevent temporal drift).
    /// In the end, we'll do intra-coding at the beginning of each dtm interval, so there's a guarantee
    /// that each pixel will have an event in the first block.
    pub(crate) fn forward_inter_prediction(
        &mut self,
        mut sparam: u8,
        dtm: DeltaT,
        dt_ref: DeltaT,
        events: &BlockEvents,
    ) -> (&[DResidual; 256], &[i16; 256], u8) {
        self.reset_residuals();
        let mut max_t_resid = 0;
        for ((idx, event_opt), event_mem) in
            events.iter().enumerate().zip(self.event_memory.iter_mut())
        {
            if let Some(next) = event_opt {
                // Get the d-residual
                let d_resid = d_residual(event_mem.d, next.d);
                event_mem.d = next.d; // ??? TODO
                self.d_residuals[idx] = d_resid;

                let tmp = self.t_memory[idx];

                // The true delta_t
                let delta_t = next.t() - self.t_memory[idx];

                assert!(delta_t <= dtm);

                self.t_memory[idx] = next.t();
                if self.time_modulation_mode == FramePerfect && self.t_memory[idx] % dt_ref != 0 {
                    self.t_memory[idx] = ((self.t_memory[idx] / dt_ref) + 1) * dt_ref;
                    debug_assert_eq!(self.t_memory[idx] % dt_ref, 0);
                }

                let dt_pred = predict_delta_t(event_mem, d_resid, dtm);

                // event_mem.delta_t = delta_t; // ???? TODO

                let dt_pred_residual = delta_t as DeltaTResidual - dt_pred as DeltaTResidual;
                self.dt_pred_residuals[idx] = dt_pred_residual;
                if dt_pred_residual.abs() > max_t_resid {
                    max_t_resid = dt_pred_residual.abs();
                    assert!(max_t_resid <= dtm as DeltaTResidual);
                    assert!(max_t_resid < 100000000);
                }
            }
        }

        // if max_t_resid is greater than 2^15, then we need to increase the sparam
        let num_places = max_t_resid.leading_zeros();
        if num_places + (sparam as u32) < 49 && max_t_resid > 0 {
            sparam = (49 - num_places) as u8;
        }

        // Quantize the T residuals
        for (t_resid, t_resid_i16) in self
            .dt_pred_residuals
            .iter()
            .zip(self.dt_pred_residuals_i16.iter_mut())
        {
            *t_resid_i16 = (*t_resid >> sparam) as i16;
            // assert!(t_resid_i16.abs() <= dtm as i16);
        }

        self.reconstruct_t_values(sparam, dtm, dt_ref);

        (&self.d_residuals, &self.dt_pred_residuals_i16, sparam)
    }

    pub(crate) fn inverse_inter_prediction(
        &mut self,
        sparam: u8,
        dtm: DeltaT,
        dt_ref: DeltaT,
    ) -> [Option<EventCoordless>; BLOCK_SIZE_AREA] {
        let mut events = [None; BLOCK_SIZE_AREA];
        for (idx, ((d_resid, t_resid_i16), event_mem)) in self
            .d_residuals
            .iter()
            .zip(self.dt_pred_residuals_i16)
            .zip(self.event_memory.iter_mut())
            .enumerate()
        {
            if *d_resid != D_ENCODE_NO_EVENT as i16 {
                let d = (event_mem.d as DResidual + *d_resid) as D;
                // let mut event = EventCoordless { d, delta_t: 0 }
                let t_resid = ((t_resid_i16 as DeltaTResidual) << sparam);
                let mut dt_pred = match *d_resid > 0 {
                    true => {
                        if *d_resid < 8 {
                            event_mem.delta_t << *d_resid
                        } else {
                            event_mem.delta_t
                        }
                    }
                    false => {
                        if *d_resid > -8 {
                            event_mem.delta_t >> -*d_resid
                        } else {
                            event_mem.delta_t
                        }
                    }
                };
                if dt_pred > dtm {
                    dt_pred = event_mem.delta_t;
                }
                // if dt_pred > dtm as DeltaTResidual {
                //     dt_pred = event_mem.delta_t as DeltaTResidual;
                // }

                let recon_t = (self.t_recon[idx] as DeltaTResidual
                    + dt_pred as DeltaTResidual
                    + t_resid) as DeltaT;
                event_mem.delta_t = recon_t - self.t_recon[idx];
                event_mem.d = d;
                self.t_recon[idx] = recon_t;
                if self.time_modulation_mode == FramePerfect && self.t_recon[idx] % dt_ref != 0 {
                    self.t_recon[idx] = ((self.t_recon[idx] / dt_ref) + 1) * dt_ref;
                }

                let event = EventCoordless {
                    d,
                    delta_t: recon_t,
                };
                events[idx] = Some(event);
            }
        }

        events
    }

    fn reconstruct_t_values(&mut self, sparam: u8, dtm: DeltaT, dt_ref: DeltaT) {
        for ((event_mem, t_resid_i16), (idx, d_resid)) in self
            .event_memory
            .iter_mut()
            .zip(self.dt_pred_residuals_i16.iter())
            .zip(self.d_residuals.iter().enumerate())
        {
            if *d_resid != D_ENCODE_NO_EVENT {
                let dt_pred_residual = ((*t_resid_i16 as DeltaTResidual) << sparam);

                let dt_pred = predict_delta_t(event_mem, *d_resid, dtm);

                update_values_from_prediction(
                    event_mem,
                    &mut self.t_recon[idx],
                    dt_pred,
                    dt_pred_residual,
                    dtm,
                );

                if self.time_modulation_mode == FramePerfect && self.t_recon[idx] % dt_ref != 0 {
                    self.t_recon[idx] = ((self.t_recon[idx] / dt_ref) + 1) * dt_ref;
                }
            }
        }
    }
}

#[inline(always)]
fn d_residual(d0: D, d1: D) -> DResidual {
    d1 as DResidual - d0 as DResidual
}

#[inline(always)]
fn t_residual(t0: AbsoluteT, t1: AbsoluteT) -> TResidual {
    t1 - t0
}

#[inline(always)]
fn delta_t_residual(t0: DeltaTResidual, t1: DeltaTResidual) -> DeltaTResidual {
    t1 - t0
}

#[inline]
fn predict_delta_t(event_memory: &mut EventCoordless, d_resid: DResidual, dtm: DeltaT) -> DeltaT {
    let mut dt_pred = match d_resid > 0 {
        true => {
            if d_resid < 8 {
                event_memory.delta_t << d_resid
            } else {
                event_memory.delta_t
            }
        }
        false => {
            if d_resid > -8 {
                event_memory.delta_t >> -d_resid
            } else {
                event_memory.delta_t
            }
        }
    };
    if dt_pred > dtm {
        dt_pred = event_memory.delta_t;
    }
    dt_pred
}

fn update_values_from_prediction(
    event_memory: &mut EventCoordless,
    t_recon: &mut AbsoluteT,
    dt_pred: DeltaT,
    dt_pred_residual: DeltaTResidual,
    dtm: DeltaT,
) {
    let recon_t =
        (*t_recon as DeltaTResidual + dt_pred as DeltaTResidual + dt_pred_residual) as AbsoluteT;
    event_memory.delta_t = recon_t - *t_recon;
    assert!(event_memory.delta_t <= dtm);
    // self.event_memory[idx].d = d; TODO?
    *t_recon = recon_t;
}