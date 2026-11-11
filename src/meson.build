cargo_options = [
  '--manifest-path', meson.project_source_root() / 'Cargo.toml',
  '--target-dir', meson.project_build_root() / 'target',
]
cargo_env = [
  'FINARYX_APP_ID=' + application_id,
  'FINARYX_LOCALEDIR=' + localedir,
  'FINARYX_PROFILE=' + profile,
]

if get_option('profile') == 'default'
  cargo_options += ['--release']
  rust_target = 'release'
  message('Building in release mode')
else
  rust_target = 'debug'
  message('Building in debug mode')
endif

cargo_build = custom_target(
  'cargo-build',
  build_by_default: true,
  build_always_stale: true,
  output: meson.project_name(),
  console: true,
  install: true,
  install_dir: bindir,
  command: [
    'env', cargo_env,
    cargo, 'build', cargo_options,
    '&&',
    'cp', meson.project_build_root() / 'target' / rust_target / meson.project_name(), '@OUTPUT@',
  ],
)

test(
  'cargo-test',
  cargo,
  args: ['test', '--workspace', cargo_options],
  env: cargo_env,
  timeout: 1200,
  is_parallel: false,
)
