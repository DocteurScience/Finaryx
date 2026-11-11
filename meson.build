project(
  'finaryx',
  'rust',
  version: '0.1.0',
  license: 'AGPL-3.0-or-later',
  meson_version: '>= 1.0',
)

i18n = import('i18n')
gnome = import('gnome')

base_id = 'io.github.docteurscience.Finaryx'

dependency('glib-2.0', version: '>= 2.76')
dependency('gtk4', version: '>= 4.14')
dependency('libadwaita-1', version: '>= 1.5')

cargo = find_program('cargo', required: true)
glib_compile_schemas = find_program('glib-compile-schemas', required: true)
desktop_file_validate = find_program('desktop-file-validate', required: false)
appstreamcli = find_program('appstreamcli', required: false)

if get_option('profile') == 'development'
  profile = 'Devel'
  application_id = '@0@.@1@'.format(base_id, profile)
else
  profile = ''
  application_id = base_id
endif

prefix = get_option('prefix')
bindir = prefix / get_option('bindir')
localedir = prefix / get_option('localedir')
datadir = prefix / get_option('datadir')
gettext_package = meson.project_name()

subdir('data')
subdir('po')
subdir('src')

gnome.post_install(
  gtk_update_icon_cache: true,
  glib_compile_schemas: true,
  update_desktop_database: true,
)
