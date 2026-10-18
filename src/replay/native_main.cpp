/*
 * MacNeutron: dxmt-replay's main in the native build (a VM host's): its arguments as wmain takes them.
 */
#include <cstdlib>
#include <string>
#include <vector>

int wmain(int argc, wchar_t **argv);

int
main(int argc, char **argv) {
  std::vector<std::wstring> wide(argc);
  std::vector<wchar_t *> args(argc + 1, nullptr);
  for (int i = 0; i < argc; i++) {
    size_t n = mbstowcs(nullptr, argv[i], 0);
    wide[i].resize(n == (size_t)-1 ? 0 : n);
    if (n != (size_t)-1)
      mbstowcs(wide[i].data(), argv[i], n + 1);
    args[i] = wide[i].data();
  }
  return wmain(argc, args.data());
}
