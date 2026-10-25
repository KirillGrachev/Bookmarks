//! Точка входа десктопного приложения Bookmarks.
//!
//! Бинарник минимален: вся логика живёт в `bookmarks_lib`, чтобы её же можно
//! было использовать из тестов и из мобильной сборки Tauri.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    bookmarks_lib::run();
}
