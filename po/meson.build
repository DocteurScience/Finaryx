i18n.gettext(
  gettext_package,
  preset: 'glib',
  args: ['--keyword=N_', '--keyword=ngettext_n:1,2', '--keyword=n_text:1,2', '--add-comments=TRANSLATORS', '--from-code=UTF-8'],
)

# The translation table and the generated .po files (see po/README.md).
python = find_program('python3')
test(
  'translations',
  python,
  args: [meson.project_source_root() / 'build-aux' / 'translations.py', 'check'],
)
msgfmt = find_program('msgfmt', required: false)
if msgfmt.found()
  foreach lang : run_command('cat', 'LINGUAS', check: true).stdout().split()
    test(
      'msgfmt-' + lang,
      msgfmt,
      args: ['--check', '--output-file=/dev/null', meson.current_source_dir() / lang + '.po'],
    )
  endforeach
endif
