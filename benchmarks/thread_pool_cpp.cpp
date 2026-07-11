// Fixed-capacity C++ thread-pool scheduler fork/join benchmark.
#include <atomic>
#include <chrono>
#include <cstddef>
#include <iostream>

#include <boost/sml/utility/thread_pool_scheduler.hpp>

namespace policy = boost::sml::utility::policy;

int main() {
#if BOOST_SML_UTILITY_THREAD_POOL_SCHEDULER_ENABLED
  constexpr std::size_t workers = 8;
  constexpr std::uint64_t rounds = 5'000;
  using pool_type = policy::thread_pool_scheduler<workers>;
  using scheduler_type = policy::thread_pool_scheduler_ref<pool_type>;

  pool_type pool{};
  scheduler_type scheduler{pool};
  std::atomic<std::uint64_t> calls{0};

  const auto start = std::chrono::steady_clock::now();
  for (std::uint64_t round = 0; round < rounds; ++round) {
    scheduler_type::join_group group{};
    for (std::size_t lane = 0; lane < workers; ++lane) {
      if (!scheduler.try_submit(group, [&calls] {
        calls.fetch_add(1, std::memory_order_relaxed);
      })) {
        std::cerr << "task submission failed\n";
        return 3;
      }
    }
    if (!group.wait()) {
      std::cerr << "join group rejected a task\n";
      return 4;
    }
  }
  const auto nanoseconds = std::chrono::duration_cast<std::chrono::nanoseconds>(
                               std::chrono::steady_clock::now() - start)
                               .count();
  const auto expected = rounds * workers;
  if (calls.load(std::memory_order_acquire) != expected) {
    std::cerr << "incorrect completed-task count\n";
    return 5;
  }
  std::cout << "cpp-thread-pool " << nanoseconds << " ns total; "
            << static_cast<double>(nanoseconds) / static_cast<double>(expected)
            << " ns/task\n";
#else
  std::cerr << "thread_pool_scheduler requires C++20 and <semaphore>\n";
  return 2;
#endif
}
