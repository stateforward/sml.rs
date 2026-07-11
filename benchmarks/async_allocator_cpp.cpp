// C++20 `co_sm` allocator-policy benchmark for the canonical player workload.
#include <chrono>
#include <cstdlib>
#include <iostream>
#include <string_view>

#include <boost/sml.hpp>
#include <boost/sml/utility/co_sm.hpp>

#include "sml_player_sm.hpp"

namespace policy = boost::sml::utility::policy;

struct forced_inline_scheduler {
  static constexpr bool guarantees_fifo = true;
  static constexpr bool single_consumer = true;
  static constexpr bool run_to_completion = true;

  template <class Fn>
  void schedule(Fn&& fn) noexcept {
    static_cast<Fn&&>(fn)();
  }
};

template <class Machine>
void dispatch_workload(Machine& machine) {
  for (int i = 0; i < 1'000'000; ++i) {
#define DISPATCH(Event)                                      \
  machine.process_event_async(Event{}).result();             \
  asm volatile("" : : "g"(&machine) : "memory")
    DISPATCH(open_close);
    DISPATCH(open_close);
    DISPATCH(cd_detected);
    DISPATCH(play);
    DISPATCH(pause);
    DISPATCH(end_pause);
    DISPATCH(pause);
    DISPATCH(stop);
    DISPATCH(stop);
    DISPATCH(open_close);
    DISPATCH(open_close);
#undef DISPATCH
  }
}

template <class Machine>
void measure(const char* name) {
  Machine machine{};
  const auto start = std::chrono::steady_clock::now();
  dispatch_workload(machine);
  const auto nanoseconds = std::chrono::duration_cast<std::chrono::nanoseconds>(
                               std::chrono::steady_clock::now() - start)
                               .count();
  // Only Empty accepts cd_detected, so this validates the final state without
  // relying on the table's function-local state type.
  if (!machine.process_event_async(cd_detected{}).result()) std::abort();
  std::cout << name << ' ' << nanoseconds << " ns total; "
            << static_cast<double>(nanoseconds) / 11'000'000.0 << " ns/event\n";
}

int main(int argc, char** argv) {
  using inline_machine = boost::sml::utility::co_sm<
      player, policy::coroutine_scheduler<policy::inline_scheduler>>;
  using pooled_machine = boost::sml::utility::co_sm<
      player, policy::coroutine_scheduler<forced_inline_scheduler>,
      policy::coroutine_allocator<policy::pooled_coroutine_allocator<>>>;
  using heap_machine = boost::sml::utility::co_sm<
      player, policy::coroutine_scheduler<forced_inline_scheduler>,
      policy::coroutine_allocator<policy::heap_coroutine_allocator>>;

  const std::string_view mode = argc > 1 ? argv[1] : "all";
  if (mode == "inline" || mode == "all") measure<inline_machine>("cpp-inline");
  if (mode == "pooled" || mode == "all") measure<pooled_machine>("cpp-pooled");
  if (mode == "heap" || mode == "all") measure<heap_machine>("cpp-heap");
}
