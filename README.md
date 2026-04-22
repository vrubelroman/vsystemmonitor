# vsysmonitor

TUI-монитор системных ресурсов для Linux на Rust. Текущая реализация закрывает этап 1 из ТЗ: локальный хост, CPU/RAM/Disk, конфиг, горячие клавиши и paging-архитектуру под будущие удаленные хосты.

## Что уже заложено

- единая модель `HostInfo` для local/remote;
- слой `collector` с локальным сборщиком как первой реализацией;
- UI, который рендерит список хостов и умеет page-based navigation;
- конфиг через XDG-путь `~/.config/vsysmonitor/config.toml`;
- тема по умолчанию: `Catppuccin Mocha`.

## Структура

- [src/app.rs](/home/vrubel/projects/vsysmonitor/src/app.rs)
- [src/config.rs](/home/vrubel/projects/vsysmonitor/src/config.rs)
- [src/model.rs](/home/vrubel/projects/vsysmonitor/src/model.rs)
- [src/navigation.rs](/home/vrubel/projects/vsysmonitor/src/navigation.rs)
- [src/collector/local.rs](/home/vrubel/projects/vsysmonitor/src/collector/local.rs)
- [src/ui.rs](/home/vrubel/projects/vsysmonitor/src/ui.rs)
- [assets/config.example.toml](/home/vrubel/projects/vsysmonitor/assets/config.example.toml)

## Сборка

Когда Rust toolchain установлен:

```bash
cargo run
```

Чтобы использовать свой конфиг:

```bash
mkdir -p ~/.config/vsysmonitor
cp assets/config.example.toml ~/.config/vsysmonitor/config.toml
```

## Закладка под пакетирование

Проект сознательно сделан как обычный `cargo`-пакет без привязки к конкретной системе сборки. Это упростит:

- публикацию исходников и инструкции для GitHub Releases;
- добавление `PKGBUILD` для Arch;
- добавление `flake.nix` или `default.nix` для NixOS;
- сборку `.deb` позднее через отдельный packaging-layer, не меняя код приложения.
