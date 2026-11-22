// Server settings: Discord's guild administration. `api` builds each REST call, `access` decides which
// pages and controls the signed-in member may use, `audit` turns audit-log entries into sentences;
// `view` draws the window on the shared settings shell, with `roles`, `lists` and `members` for their pages, and
// `registry` lists every setting for the window's search.
pub mod access;
pub mod api;
pub mod audit;
#[cfg(debug_assertions)]
pub mod demo;
pub mod lists;
pub mod members;
pub mod perms;
pub mod registry;
pub mod roles;
pub mod view;
