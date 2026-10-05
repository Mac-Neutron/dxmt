dxmt_src = [
  'dxmt_format.cpp',
  'dxmt_names.cpp',
  'dxmt_command_queue.cpp',
  'dxmt_command.cpp',
  'dxmt_capture.cpp',
  'dxmt_info.cpp',
  'dxmt_device.cpp',
  'dxmt_buffer.cpp',
  'dxmt_texture.cpp',
  'dxmt_context.cpp',
  'dxmt_dynamic.cpp',
  'dxmt_staging.cpp',
  'dxmt_hud_state.cpp',
  'dxmt_allocation.cpp',
  'dxmt_presenter.cpp',
  'dxmt_sampler.cpp',
  'dxmt_resource_initializer.cpp',
  'dxmt_shader_cache.cpp',
  'dxmt_scaler.cpp',
  'dxmt_subresource.cpp',
  'dxmt_deptrack.cpp',
  'dxmt_counter.cpp',
  'dxmt_fence.cpp',
  'dxmt_keyed_mutex.cpp',
]

dxmt_shaders = [
  'dxmt_command.metal'
]

dxmt_shaders_ir = metalir_generator.process(dxmt_shaders)

dxmt_shaders_lib = metallib_generator.process(dxmt_shaders_ir)

dxmt_shaders_header = hexdump_generator.process(dxmt_shaders_lib)

# MacNeutron: the shader translator's key (wine-arm64/build.sh) keys the shader cache when given. Read from a file, so
# a new key regenerates the header (fs.read makes the file a regeneration dependency) without new setup options.
translator_key_file = get_option('translator_key_file')
translator_conf = configuration_data()
translator_conf.set_quoted('DXMT_TRANSLATOR_KEY', translator_key_file == '' ? '' : fs.read(translator_key_file).strip())
configure_file(output : 'dxmt_translator_key.h', configuration : translator_conf)

dxmt_lib = static_library('dxmt', dxmt_src, dxmt_shaders_header, dxmt_version,
  dependencies        : [ util_dep, airconv_forward_dep, winemetal_dep ],
  include_directories : [ dxmt_include_path ],
)

dxmt_dep = declare_dependency(
  link_with           : [ dxmt_lib ],
  include_directories : [ dxmt_include_path, include_directories('.') ],
)
