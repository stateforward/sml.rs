// State-machine sm_pool benchmark matched to examples/sm_pool_benchmark.rs.
#include <boost/sml.hpp>
#include <boost/sml/utility/sm_pool.hpp>

#include <chrono>
#include <cstddef>
#include <cstdint>
#include <iostream>
#include <string_view>
#include <vector>

namespace sml = boost::sml;

constexpr std::size_t actors = 10'000;
constexpr std::size_t dispatches = 50'000;
constexpr std::size_t rounds = 1'001;

struct pulse {};
using indexed_pulse = sml::utility::indexed_event<pulse>;

std::vector<std::size_t> make_ids(const bool random) {
  std::vector<std::size_t> result;
  result.reserve(dispatches);
  std::uint32_t state = 1'337;
  for (std::size_t index = 0; index < dispatches; ++index) {
    if (!random) {
      result.push_back(index % actors);
      continue;
    }
    state ^= state << 13;
    state ^= state >> 17;
    state ^= state << 5;
    result.push_back(state % actors);
  }
  return result;
}

struct storage {
  explicit storage(const std::size_t count) : flags(count) {}
  void reset() { std::fill(flags.begin(), flags.end(), std::uint8_t{}); }
  std::vector<std::uint8_t> flags;
};

struct router {
  auto operator()() const {
    using namespace sml;
    const auto toggle = [](storage& data, const indexed_pulse& event) {
      data.flags[event.id] ^= std::uint8_t{1};
    };
    return make_transition_table(*"active"_s + event<indexed_pulse> / toggle);
  }
};

template <class T>
inline void do_not_optimize(const T& value) {
  asm volatile("" : : "r,m"(std::addressof(value)) : "memory");
}

void report(const std::string_view label, const std::uint64_t elapsed,
            const std::vector<std::uint8_t>& flags) {
  std::size_t checksum = 0;
  for (const auto flag : flags) checksum += flag;
  if (checksum == 0) std::terminate();
  const auto events = static_cast<double>(rounds * dispatches);
  std::cout << label << ' ' << elapsed << " ns total; " << (elapsed / events)
            << " ns/event; checksum " << checksum << '\n';
}

void measure_direct(const std::vector<std::size_t>& ids, const std::string_view label) {
  std::vector<std::uint8_t> flags(actors);
  const auto started = std::chrono::steady_clock::now();
  for (std::size_t round = 0; round < rounds; ++round) {
    for (const auto id : ids) flags[id] ^= std::uint8_t{1};
  }
  const auto elapsed = std::chrono::duration_cast<std::chrono::nanoseconds>(
                           std::chrono::steady_clock::now() - started)
                           .count();
  do_not_optimize(flags);
  report(label, static_cast<std::uint64_t>(elapsed), flags);
}

void measure_pool(const std::vector<std::size_t>& ids, const std::string_view label,
                  const bool batch) {
  sml::utility::sm_pool<storage, router> pool(actors);
  const auto started = std::chrono::steady_clock::now();
  for (std::size_t round = 0; round < rounds; ++round) {
    if (batch) {
      do_not_optimize(pool.process_indexed_batch<pulse>(ids));
    } else {
      for (const auto id : ids) do_not_optimize(pool.process_indexed<pulse>(id));
    }
  }
  const auto elapsed = std::chrono::duration_cast<std::chrono::nanoseconds>(
                           std::chrono::steady_clock::now() - started)
                           .count();
  report(label, static_cast<std::uint64_t>(elapsed), pool.storage().flags);
}

int main(const int argc, const char* const* argv) {
  const std::string_view mode = argc > 1 ? argv[1] : "all";
  const auto local = make_ids(false);
  const auto random = make_ids(true);
  if (mode == "direct-local" || mode == "all") measure_direct(local, "cpp-direct-local");
  if (mode == "direct-random" || mode == "all") measure_direct(random, "cpp-direct-random");
  if (mode == "scalar-local" || mode == "all")
    measure_pool(local, "cpp-pool-scalar-local", false);
  if (mode == "scalar-random" || mode == "all")
    measure_pool(random, "cpp-pool-scalar-random", false);
  if (mode == "batch-local" || mode == "all") measure_pool(local, "cpp-pool-batch-local", true);
  if (mode == "batch-random" || mode == "all")
    measure_pool(random, "cpp-pool-batch-random", true);
}
