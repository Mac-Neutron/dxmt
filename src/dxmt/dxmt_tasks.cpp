#include "dxmt_tasks.hpp"
#include <algorithm>
#include <chrono>
#include <mutex>
#include <vector>

namespace dxmt {

// MacNeutron: see dxmt_tasks.hpp.
static std::mutex schedulers_lock;
static std::vector<task_scheduler_base *> schedulers;

void
task_scheduler_base::enlist() {
  std::lock_guard<std::mutex> lock(schedulers_lock);
  schedulers.push_back(this);
}

void
task_scheduler_base::delist() {
  std::lock_guard<std::mutex> lock(schedulers_lock);
  schedulers.erase(std::remove(schedulers.begin(), schedulers.end(), this), schedulers.end());
}

bool
wait_schedulers_idle(unsigned timeout_ms) {
  auto end = std::chrono::steady_clock::now() + std::chrono::milliseconds(timeout_ms);
  for (;;) {
    bool all = true;
    {
      std::lock_guard<std::mutex> lock(schedulers_lock);
      for (auto *s : schedulers)
        all = all && s->idle();
    }
    if (all)
      return true;
    if (std::chrono::steady_clock::now() >= end)
      return false;
    Sleep(1);
  }
}

} // namespace dxmt
