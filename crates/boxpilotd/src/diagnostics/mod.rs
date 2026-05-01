//! Diagnostics export pipeline (spec §5.5 / §14, plan #8). The public entry
//! point is [`compose`], called from `iface::diagnostics_export_redacted`.

pub mod bundle;
pub mod gc;
pub mod sysinfo;
