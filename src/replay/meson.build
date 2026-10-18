# dxmt-replay.exe (MacNeutron): rebuilds a game's recorded D3D12 pipelines; see dxmt_replay.cpp.
if dxmt_crossbuild
executable('dxmt-replay', [ 'dxmt_replay.cpp', '../d3d12/d3d12_pipeline_record.cpp' ],
  dependencies        : [ util_dep ],
  include_directories : [ dxmt_include_path, include_directories('../d3d12') ],
  link_args           : [ '-municode' ],
  install             : true,
  install_dir         : windows_native_install_dir,
)
else
# MacNeutron, natively: a VM host replays a game's recordings before its VM boots, into native DXMT's cache.
executable('dxmt-replay', [ 'dxmt_replay.cpp', 'native_main.cpp', '../d3d12/d3d12_pipeline_record.cpp' ],
  dependencies        : [ util_dep ],
  include_directories : [ dxmt_include_path, include_directories('../d3d12') ],
  install             : false,
)
endif
