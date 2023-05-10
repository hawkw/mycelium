#![cfg_attr(docsrs, doc = include_str!("../README.md"))]
#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg, doc_cfg_hide))]
#![cfg_attr(not(test), no_std)]

#[doc(inline)]
pub use self::{
    addr::Address,
    class::{Class, Classes, Subclass},
    device::Device,
};
pub mod addr;
pub mod class;
pub mod config;
pub mod device;
pub mod error;
pub mod express;
pub mod register;
