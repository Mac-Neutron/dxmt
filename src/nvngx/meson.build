nvngx_src = [
  'nvngx.cpp',
  'nvngx_d3d12.cpp',
]

nvngx_ld_args      = []
nvngx_link_depends = []

nvngx_dll = shared_library('nvngx', nvngx_src,
  name_prefix         : '',
  dependencies        : [ dxgi_dep, util_dep, winemetal_dep ],
  include_directories : [ dxmt_include_path ],
  install             : true,
  install_dir         : windows_native_install_dir,
  link_args           : nvngx_ld_args,
  link_depends        : [ nvngx_link_depends ],
  install_tag         : 'nvext',
)

# a wine-builtin dll where DXMT's are (MacNeutron: native in system32 otherwise, as nvapi64)
if wine_builtin_dll
custom_target('postprocess_libnvngx',
  input : nvngx_dll,
  output: 'nvngx.dll.postproc',
  command : [ winebuild, '--builtin', '@INPUT@' ],
  depends : nvngx_dll,
  build_by_default : true
)
endif
