mod git;
mod manager;
mod package;
mod source;
mod staging;
mod update;

#[cfg(test)]
mod package_tests;

pub use manager::GitPluginManager;
