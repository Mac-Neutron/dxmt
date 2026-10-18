#pragma once

#include "thread.hpp"
#include "util_win32_compat.h"
#include <atomic>
#include <queue>
#include <unordered_map>

namespace dxmt {

template <typename Task> struct task_trait {
  Task run_task(Task task);
  bool get_done(Task task);
  void set_done(Task task);
};

// MacNeutron: the process's schedulers, so a VM host can wait until none holds or runs a task before it releases what
// their tasks use (a guest process's end: its shaders and pipelines own their compile tasks).
class task_scheduler_base {
public:
  virtual bool idle() = 0;

protected:
  void enlist();
  void delist();
  ~task_scheduler_base() = default;
};
// Until every scheduler of the process is idle, at most timeout_ms: whether they all were.
bool wait_schedulers_idle(unsigned timeout_ms);

template <typename Task> class task_scheduler : public task_scheduler_base {
public:
  void submit(Task task);

  bool
  idle() override {
    {
      std::unique_lock<dxmt::mutex> lock(worker_mutex_);
      if (!task_queue_.empty() || !task_continuation_queue_.empty() || running.load())
        return false;
    }
    std::unique_lock<dxmt::mutex> lock(deps_mutex_);
    return task_continuation_.empty();
  }

  task_scheduler();
  ~task_scheduler();

  uint64_t
  get_running_threads() {
    return running.load(std::memory_order_relaxed);
  }

private:
  void worker_func();

  dxmt::mutex worker_mutex_;
  dxmt::condition_variable worker_cond_;
  std::queue<Task> task_queue_;
  std::queue<Task> task_continuation_queue_;

  dxmt::mutex deps_mutex_;
  std::unordered_multimap<Task, Task> task_continuation_;

  std::vector<dxmt::thread> workers_;

  std::atomic_bool destroyed = false;
  std::atomic_uint64_t running = 0;
  uint64_t threads;
  uint64_t max_threads;
};

template <typename Task> task_scheduler<Task>::task_scheduler() {
  max_threads = dxmt::thread::hardware_concurrency() * 2;
  workers_.reserve(max_threads);
  threads = 2;

  for (unsigned i = 0; i < threads; i++) {
    workers_.emplace_back([this]() { worker_func(); });
  }
  enlist();
}

template <typename Task> task_scheduler<Task>::~task_scheduler() {
  delist();
  destroyed.store(true);
  worker_cond_.notify_all();

  for (auto &worker : workers_) {
    if (worker.joinable())
      worker.join();
  }

  workers_.clear();
}

template <typename Task>
void
task_scheduler<Task>::worker_func() {
  struct task_trait<Task> task_trait;
  std::vector<Task> continuation_buffer;
  SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_TIME_CRITICAL);
  while (!destroyed.load()) {
    Task task;
    {
      std::unique_lock<dxmt::mutex> lock(worker_mutex_);

      if (task_queue_.empty() && task_continuation_queue_.empty()) {
        worker_cond_.wait(lock, [this]() {
          return task_queue_.size() || task_continuation_queue_.size() || destroyed.load();
        });
      }

      if (!task_continuation_queue_.empty()) {
        task = task_continuation_queue_.front();
        task_continuation_queue_.pop();
      } else if (!task_queue_.empty()) {
        task = task_queue_.front();
        task_queue_.pop();
      } else {
        break;
      }
      running.fetch_add(1, std::memory_order_relaxed); // under the lock: idle() never sees a task between queue and run
    }
    while (true) {
      Task continuation = task_trait.run_task(task);
      if (continuation == task) {
        {
          std::unique_lock<dxmt::mutex> lock(deps_mutex_);
          auto range = task_continuation_.equal_range(continuation);
          for (auto itr = range.first; itr != range.second; ++itr) {
            continuation_buffer.push_back(itr->second);
          }
          task_continuation_.erase(range.first, range.second);
          task_trait.set_done(continuation);
        }
        {
          std::unique_lock<dxmt::mutex> lock(worker_mutex_);
          for (auto &task : continuation_buffer) {
            task_continuation_queue_.push(task);
          }
        }
        worker_cond_.notify_all();
        continuation_buffer.clear();
      } else {
        std::unique_lock<dxmt::mutex> lock(deps_mutex_);
        // spurious dependency
        if (task_trait.get_done(continuation)) {
          continue;
        }
        task_continuation_.insert({continuation, task});
      }
      break;
    }
    running.fetch_sub(1, std::memory_order_relaxed);
  }
};

template <typename Task>
void
task_scheduler<Task>::submit(Task task) {
  std::unique_lock<dxmt::mutex> lock(worker_mutex_);
  task_queue_.push(task);

  if (running.load(std::memory_order_relaxed) == threads && threads < max_threads) {
    workers_.emplace_back([this]() { worker_func(); });
    threads++;
  }

  worker_cond_.notify_one();
}

}; // namespace dxmt
