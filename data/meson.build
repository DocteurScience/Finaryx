# Desktop file
desktop_conf = configuration_data()
desktop_conf.set('APP_ID', application_id)
desktop_file = i18n.merge_file(
  type: 'desktop',
  input: configure_file(
    input: '@0@.desktop.in'.format(base_id),
    output: '@0@.desktop.in'.format(application_id),
    configuration: desktop_conf,
  ),
  output: '@0@.desktop'.format(application_id),
  po_dir: meson.project_source_root() / 'po',
  install: true,
  install_dir: datadir / 'applications',
)
if desktop_file_validate.found()
  test('validate-desktop', desktop_file_validate, args: [desktop_file])
endif

# Metainfo
metainfo_file = i18n.merge_file(
  input: configure_file(
    input: '@0@.metainfo.xml.in'.format(base_id),
    output: '@0@.metainfo.xml.in'.format(application_id),
    configuration: desktop_conf,
  ),
  output: '@0@.metainfo.xml'.format(application_id),
  po_dir: meson.project_source_root() / 'po',
  install: true,
  install_dir: datadir / 'metainfo',
)
if appstreamcli.found()
  test(
    'validate-metainfo',
    appstreamcli,
    args: ['validate', '--no-net', '--explain', metainfo_file],
  )
endif

# GSettings schema (same id for every profile)
install_data(
  '@0@.gschema.xml'.format(base_id),
  install_dir: datadir / 'glib-2.0' / 'schemas',
)
test(
  'validate-schema',
  glib_compile_schemas,
  args: ['--strict', '--dry-run', meson.current_source_dir()],
)

# Application icons
install_data(
  'icons/hicolor/scalable/apps/@0@.svg'.format(base_id),
  rename: '@0@.svg'.format(application_id),
  install_dir: datadir / 'icons' / 'hicolor' / 'scalable' / 'apps',
)
install_data(
  'icons/hicolor/symbolic/apps/@0@-symbolic.svg'.format(base_id),
  rename: '@0@-symbolic.svg'.format(application_id),
  install_dir: datadir / 'icons' / 'hicolor' / 'symbolic' / 'apps',
)
