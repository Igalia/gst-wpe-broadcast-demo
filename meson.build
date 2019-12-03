project(
  'gst-wpe-broadcast-demo', 'rust',
  version: '1.0.0',
  license: 'MIT',
)

gst_wpe_broadcast_demo_version = meson.project_version()
version_array = gst_wpe_broadcast_demo_version.split('.')
gst_wpe_broadcast_demo_major_version = version_array[0].to_int()
gst_wpe_broadcast_demo_minor_version = version_array[1].to_int()
gst_wpe_broadcast_demo_version_micro = version_array[2].to_int()

gst_wpe_broadcast_demo_prefix = get_option('prefix')
gst_wpe_broadcast_demo_bindir = join_paths(gst_wpe_broadcast_demo_prefix, get_option('bindir'))
gst_wpe_broadcast_demo_localedir = join_paths(gst_wpe_broadcast_demo_prefix, get_option('localedir'))

datadir = get_option('datadir')
icondir = join_paths(datadir, 'icons')

cargo = find_program('cargo')
cargo_vendor = find_program('cargo-vendor', required: false)
cargo_script = find_program('scripts/cargo.sh')
grabber = find_program('scripts/grabber.sh')
cargo_release = find_program('scripts/release.sh')

c = run_command(grabber)
sources = c.stdout().strip().split('\n')

install_data('data/com.igalia.GstWPEBroadcastDemo.desktop', install_dir : datadir + '/applications')
install_data('data/com.igalia.GstWPEBroadcastDemo.svg', install_dir : icondir + '/hicolor/scalable/apps/')
install_data('data/com.igalia.GstWPEBroadcastDemo.appdata.xml', install_dir : datadir + '/appdata/')

cargo_release = custom_target('cargo-build',
                        build_by_default: true,
                        console: true,
                        input: sources,
                        output: ['gst-wpe-broadcast-demo'],
                        install: true,
                        install_dir: gst_wpe_broadcast_demo_bindir,
                        command: [cargo_script, '@CURRENT_SOURCE_DIR@', '@OUTPUT@', gst_wpe_broadcast_demo_localedir])

run_target('release', command: ['scripts/release.sh',
                                meson.project_name() + '-' + gst_wpe_broadcast_demo_version
                               ])

meson.add_install_script('scripts/meson_post_install.py')
