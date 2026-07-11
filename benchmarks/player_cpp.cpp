// Nanosecond-resolution harness for ../sml.cpp's canonical CD-player workload.
#include <cassert>
#include <chrono>
#include <iostream>

#include "sml_player_sm.hpp"

int main() {
  sml::sm<player> machine;
  const auto start = std::chrono::steady_clock::now();
  run_player_one_million(machine);
  const auto elapsed = std::chrono::steady_clock::now() - start;
  const auto nanoseconds =
      std::chrono::duration_cast<std::chrono::nanoseconds>(elapsed).count();
  assert(machine.is(sml::state<class Empty>));
  std::cout << nanoseconds << " ns total; "
            << static_cast<double>(nanoseconds) / 11'000'000.0
            << " ns/event\n";
}
