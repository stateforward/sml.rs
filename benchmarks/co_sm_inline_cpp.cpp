// Nanosecond-resolution harness for sml.cpp's inline co_sm RTC path.
#include <cassert>
#include <chrono>
#include <iostream>

#include <boost/sml/utility/co_sm.hpp>
#include "sml_player_sm.hpp"

namespace utility = boost::sml::utility;

int main() {
  using machine_type = utility::co_sm<
      player,
      utility::policy::coroutine_scheduler<utility::policy::inline_scheduler>>;
  machine_type machine;

  const auto start = std::chrono::steady_clock::now();
  for (auto i = 0; i < 1'000'000; ++i) {
    (void)machine.process_event_async(open_close{}).result();
    asm volatile("" : : "g"(&machine) : "memory");
    (void)machine.process_event_async(open_close{}).result();
    asm volatile("" : : "g"(&machine) : "memory");
    (void)machine.process_event_async(cd_detected{}).result();
    asm volatile("" : : "g"(&machine) : "memory");
    (void)machine.process_event_async(play{}).result();
    asm volatile("" : : "g"(&machine) : "memory");
    (void)machine.process_event_async(pause{}).result();
    asm volatile("" : : "g"(&machine) : "memory");
    (void)machine.process_event_async(end_pause{}).result();
    asm volatile("" : : "g"(&machine) : "memory");
    (void)machine.process_event_async(pause{}).result();
    asm volatile("" : : "g"(&machine) : "memory");
    (void)machine.process_event_async(stop{}).result();
    asm volatile("" : : "g"(&machine) : "memory");
    (void)machine.process_event_async(stop{}).result();
    asm volatile("" : : "g"(&machine) : "memory");
    (void)machine.process_event_async(open_close{}).result();
    asm volatile("" : : "g"(&machine) : "memory");
    (void)machine.process_event_async(open_close{}).result();
    asm volatile("" : : "g"(&machine) : "memory");
  }
  const auto elapsed = std::chrono::steady_clock::now() - start;
  const auto nanoseconds =
      std::chrono::duration_cast<std::chrono::nanoseconds>(elapsed).count();

  assert(machine.is(sml::state<class Empty>));

  std::cout << nanoseconds << " ns total; "
            << static_cast<double>(nanoseconds) / 11'000'000.0
            << " ns/event\n";
}
