subdir('util')
subdir('airconv')

if not dxmt_crossbuild
subdir('nativemetal')
else
subdir('winemetal')
endif

subdir('dxmt')
subdir('dxgi')
subdir('d3d11')
subdir('d3d10')

if get_option('enable_d3d12')
subdir('d3d12')
# The replay tool: a Windows program (wmain, d3d12.dll by name), and natively a VM host's (MacNeutron: its main, and
# d3d12.dylib beside it).
subdir('replay')
endif

if get_option('enable_nvapi')
subdir('nvapi')
endif

if get_option('enable_nvngx')
subdir('nvngx')
endif