/// Tools for transcoding from a DVS/DAVIS video source to ADΔER
pub mod davis;

/// Tools for transcoding from a framed video source to ADΔER
pub mod framed;

/// Common functions and structs for all transcoder sources
pub mod video;

/// Constant Rate Factor lookup table
#[rustfmt::skip]
pub static CRF: [[f32; 5]; 10] = [ 
// baseline C     max C    Dt_max mutliplier    C increase velocity             feature radius
//                           (X*dt_ref)    (+1 C every X*dt_ref time)   (X * min resolution, in pixels)
/*0*/    [0.0,     0.0,         20.0,                10.0,                     1E-9],
/*1*/    [0.0,     3.0,         25.0,                 9.0,                     1.0/12.0],
/*2*/    [1.0,     5.0,         30.0,                 8.0,                     1.0/15.0],
/*3*/    [3.0,     7.0,         35.0,                 7.0,                     1.0/18.0],
/*4*/    [5.0,    9.0,         40.0,                 6.0,                     1.0/20.0],
/*5*/    [7.0,    10.0,         45.0,                 5.0,                     1.0/23.0],
/*6*/    [9.0,    15.0,         50.0,                 4.0,                     1.0/26.0],
/*7*/    [11.0,    20.0,         55.0,                 3.0,                     1.0/30.0],
/*8*/    [13.0,    30.0,         60.0,                 2.0,                     1.0/35.0],
/*9*/    [15.0,   40.0,         65.0,                 1.0,                     1.0/40.0],
];

/// The default CRF quality level
pub const DEFAULT_CRF_QUALITY: u8 = 3;
