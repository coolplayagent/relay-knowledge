//! Module writes that can replace attribute dispatch used by overload lookup.
//! A module-level __getattr__ handles missing names; __class__ can install an
//! arbitrary ModuleType subclass. Ordinary member writes retain their identity.
pub(super) const MODULE_DISPATCH_HOOKS: &[&str] = &["__getattr__", "__class__"];
