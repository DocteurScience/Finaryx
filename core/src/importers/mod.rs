//! Generic CSV importer (specification §3): no code per bank. The file
//! format is detected, the user maps the columns, and bank "models" are
//! only saved mappings (profiles).

pub mod bank_categories;
pub mod format;
pub mod mapping;
pub mod plan;
pub mod profiles;
