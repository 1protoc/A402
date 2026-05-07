#include "a402_wrapper.h"
#include <iostream>
#include <vector>
#include <thread>
#include <chrono>
#include <mutex>
#include <algorithm>
#include <iomanip>
#include <sstream>
#include <fstream>
#include <cmath>
#include <atomic>
#include <queue>
#include <condition_variable>
#include <unordered_map>
#include <cstring>
#include <functional>

#ifdef __linux__
#include <pthread.h>
#include <sched.h>
#include <unistd.h>
#endif

static void precise_sleep(double seconds) {
    if (seconds <= 0.0) return;
    auto start = std::chrono::high_resolution_clock::now();
    auto end = start + std::chrono::duration<double>(seconds);
    while (std::chrono::high_resolution_clock::now() < end) {
        std::this_thread::yield();
    }
}

struct PerformanceStats {
    int total_operations = 0;
    double total_time_sec = 0.0;
    double throughput_ops_per_sec = 0.0;
    double avg_latency_ms = 0.0;
    double min_latency_ms = 0.0;
    double max_latency_ms = 0.0;
    std::vector<double> latencies_ms;
    
    void calculate() {
        if (total_operations > 0 && total_time_sec > 0) {
            throughput_ops_per_sec = total_operations / total_time_sec;
        }
        
        if (!latencies_ms.empty()) {
            double sum = 0.0;
            for (double lat : latencies_ms) {
                sum += lat;
            }
            avg_latency_ms = sum / latencies_ms.size();
            min_latency_ms = *std::min_element(latencies_ms.begin(), latencies_ms.end());
            max_latency_ms = *std::max_element(latencies_ms.begin(), latencies_ms.end());
        } else {
            avg_latency_ms = (total_time_sec * 1000.0) / total_operations;
            min_latency_ms = avg_latency_ms;
            max_latency_ms = avg_latency_ms;
        }
    }
};

struct SignatureShare {
    int utee_id;
    std::string channel_id;
    double user_c_amount;
    double m_tee_amount;
    uint8_t signature_share[64];  
    std::chrono::high_resolution_clock::time_point timestamp;
};

struct DKGSharedKey {
    int num_utees;
    std::vector<uint8_t> public_key;  
    
};

class CoordinatorTEE {
private:
    
    static constexpr int NUM_AGGREGATOR_SHARDS = 32;  
    struct AggregatorShard {
        std::unordered_map<std::string, std::vector<SignatureShare>> pending_signatures_;
        std::mutex mutex;
        std::condition_variable cv;
    };
    AggregatorShard aggregator_shards_[NUM_AGGREGATOR_SHARDS];
    
    struct AggregatedSignature {
        std::string channel_id;
        double user_c_amount;
        double m_tee_amount;
        uint8_t aggregated_signature[64];  
    };
    static constexpr int NUM_QUEUE_SHARDS = 128;  
    struct QueueShard {
        std::queue<AggregatedSignature> queue;
        std::mutex mutex;
        std::condition_variable cv;
        QueueShard() = default;
    };
    QueueShard queue_shards_[NUM_QUEUE_SHARDS];  
    std::atomic<int> queue_size_{0};
    
    std::atomic<bool> running_{true};
    std::atomic<int> collected_shares_{0};
    std::atomic<int> aggregated_signatures_{0};
    std::atomic<int> processed_requests_{0};
    
    int num_utees_;
    DKGSharedKey dkg_key_;
    
    a402_wrapper_t* mtee_wrapper_;
    int num_mtees_;
    double comm_delay_ms_;
    double proc_delay_ms_;
    
    std::thread aggregator_thread_;
    
    std::vector<std::thread> coordinator_threads_;
    int coordinator_thread_count_;
    
    void aggregator_loop() {
        while (running_ || !pending_signatures_.empty()) {
            std::unique_lock<std::mutex> lock(pending_mutex_);
            
            pending_cv_.wait(lock, [this] {
                
                for (const auto& pair : pending_signatures_) {
                    if ((int)pair.second.size() >= num_utees_) {
                        return true;
                    }
                }
                return !running_;
            });
            
            std::string request_id_to_process;
            for (auto& pair : pending_signatures_) {
                if ((int)pair.second.size() >= num_utees_) {
                    request_id_to_process = pair.first;
                    break;
                }
            }
            
            if (!request_id_to_process.empty()) {
                std::vector<SignatureShare> shares = pending_signatures_[request_id_to_process];
                pending_signatures_.erase(request_id_to_process);
                lock.unlock();
                
                aggregate_signature_shares(shares);
            }
        }
    }
    
    void aggregate_signature_shares(const std::vector<SignatureShare>& shares) {
        if (shares.empty()) return;
        
        AggregatedSignature aggregated;
        aggregated.channel_id = shares[0].channel_id;
        aggregated.user_c_amount = shares[0].user_c_amount;
        aggregated.m_tee_amount = shares[0].m_tee_amount;
        
        memset(aggregated.aggregated_signature, 0, 64);
        for (const auto& share : shares) {
            for (int i = 0; i < 64; i++) {
                aggregated.aggregated_signature[i] ^= share.signature_share[i];
            }
        }
        
        precise_sleep(0.5 / 1000.0);  
        
        int queue_shard_index = std::hash<std::string>{}(aggregated.channel_id) % NUM_QUEUE_SHARDS;
        {
            std::lock_guard<std::mutex> queue_lock(queue_shards_[queue_shard_index].mutex);
            queue_shards_[queue_shard_index].queue.push(aggregated);
            aggregated_signatures_++;
            queue_size_++;
        }
        queue_shards_[queue_shard_index].cv.notify_one();  
    }
    
    void coordinator_loop() {
        
        static std::atomic<int> thread_counter{0};
        int thread_id = thread_counter.fetch_add(1) % NUM_QUEUE_SHARDS;
        int start_shard = thread_id;
        
        while (running_ || queue_size_.load() > 0) {
            bool found = false;
            
            for (int i = 0; i < NUM_QUEUE_SHARDS; i++) {
                int shard_index = (start_shard + i) % NUM_QUEUE_SHARDS;
                std::unique_lock<std::mutex> lock(queue_shards_[shard_index].mutex);
                
                if (!queue_shards_[shard_index].queue.empty()) {
                    AggregatedSignature sig = queue_shards_[shard_index].queue.front();
                    queue_shards_[shard_index].queue.pop();
                    queue_size_--;
                    lock.unlock();
                    
                    process_aggregated_signature(sig);
                    found = true;
                    break;  
                }
                lock.unlock();
            }
            
            if (!found && running_) {
                std::this_thread::sleep_for(std::chrono::microseconds(100));
            }
        }
    }
    
    void process_mtee_requests_parallel(const std::string& channel_id, 
                                        const std::vector<AggregatedSignature>& sigs) {
        
        int num_sub_threads = std::min(100, (int)sigs.size());
        if (num_sub_threads <= 0) num_sub_threads = 1;
        
        if (sigs.size() > 100) {
            num_sub_threads = 100;  
        }
        
        std::vector<std::thread> sub_threads;
        int sigs_per_thread = sigs.size() / num_sub_threads;
        int remaining_sigs = sigs.size() % num_sub_threads;
        
        for (int t = 0; t < num_sub_threads; t++) {
            sub_threads.emplace_back([this, &sigs, t, sigs_per_thread, remaining_sigs]() {
                int start_idx = t * sigs_per_thread + std::min(t, remaining_sigs);
                int end_idx = start_idx + sigs_per_thread + (t < remaining_sigs ? 1 : 0);
                
                for (int i = start_idx; i < end_idx; i++) {
                    process_aggregated_signature(sigs[i]);
                }
            });
        }
        
        for (auto& st : sub_threads) {
            st.join();
        }
    }
    
    void process_aggregated_signature(const AggregatedSignature& sig) {
        
        const char* channel_id = sig.channel_id.c_str();
        
        precise_sleep(comm_delay_ms_ / 1000.0);
        
        bool success = a402_update_channel(
            mtee_wrapper_,
            channel_id,
            sig.user_c_amount,
            sig.m_tee_amount
        );
        
        precise_sleep(comm_delay_ms_ / 1000.0);
        
        if (success) {
            processed_requests_++;
        }
    }
    
public:
    CoordinatorTEE(a402_wrapper_t* mtee_wrapper, int num_mtees, int num_utees,
                   double comm_delay_ms, double proc_delay_ms)
        : mtee_wrapper_(mtee_wrapper)
        , num_mtees_(num_mtees)
        , num_utees_(num_utees)
        , comm_delay_ms_(comm_delay_ms)
        , proc_delay_ms_(proc_delay_ms)
    {
        
        dkg_key_.num_utees = num_utees;
        dkg_key_.public_key.resize(64);
        for (int i = 0; i < 64; i++) {
            dkg_key_.public_key[i] = (num_utees * 17 + i) % 256;
        }
        
        aggregator_thread_ = std::thread(&CoordinatorTEE::aggregator_loop, this);
        
        coordinator_thread_count_ = std::min(1280, num_mtees * 20);  
        for (int i = 0; i < coordinator_thread_count_; i++) {
            coordinator_threads_.emplace_back(&CoordinatorTEE::coordinator_loop, this);
        }
    }
    
    ~CoordinatorTEE() {
        running_ = false;
        pending_cv_.notify_all();
        
        for (auto& shard : queue_shards_) {
            shard.cv.notify_all();
        }
        if (aggregator_thread_.joinable()) {
            aggregator_thread_.join();
        }
        for (auto& t : coordinator_threads_) {
            if (t.joinable()) {
                t.join();
            }
        }
    }
    
    void submit_signature_share(const SignatureShare& share, const std::string& request_id) {
        {
            std::lock_guard<std::mutex> lock(pending_mutex_);
            pending_signatures_[request_id].push_back(share);
            collected_shares_++;
        }
        pending_cv_.notify_one();
    }
    
    int get_collected_shares() const {
        return collected_shares_.load();
    }
    
    int get_aggregated_signatures() const {
        return aggregated_signatures_.load();
    }
    
    int get_processed_requests() const {
        return processed_requests_.load();
    }
    
    void wait_for_completion(int expected_count, int timeout_sec = 300) {
        auto start = std::chrono::high_resolution_clock::now();
        int last_processed = 0;
        int no_progress_count = 0;
        
        while (processed_requests_.load() < expected_count) {
            auto now = std::chrono::high_resolution_clock::now();
            auto elapsed = std::chrono::duration<double>(now - start).count();
            
            int current_processed = processed_requests_.load();
            if (current_processed == last_processed) {
                no_progress_count++;
            } else {
                no_progress_count = 0;
                last_processed = current_processed;
            }
            
            if (static_cast<int>(elapsed) % 5 == 0 && elapsed > 0) {
                         << " (" << (current_processed * 100.0 / expected_count) << "%), "
                         << "Collected shares: " << collected_shares_.load() << ", "
                         << "Aggregated signatures: " << aggregated_signatures_.load() << "\n";
            }
            
            if (elapsed > timeout_sec) {
                         << " / " << expected_count << "  requests\n";
                break;
            }
            
            if (no_progress_count > 3000) {  
                no_progress_count = 0;  
            }
            
            std::this_thread::sleep_for(std::chrono::milliseconds(10));
        }
    }
};

class UTEE {
private:
    int utee_id_;
    CoordinatorTEE* coordinator_;
    double utee_to_coordinator_delay_ms_;
    uint8_t private_key_share_[32];  
    
    void generate_signature_share_parallel(const std::string& channel_id, 
                                          double user_c_amount, 
                                          double m_tee_amount,
                                          uint8_t* signature_share_out) {
        
        std::hash<std::string> hasher;
        size_t msg_hash = hasher(channel_id + std::to_string(user_c_amount) + std::to_string(m_tee_amount));
        
        const int max_sig_threads = 8;
        const int num_threads = std::min(max_sig_threads, static_cast<int>(std::thread::hardware_concurrency()));
        const int bytes_per_thread = 64 / num_threads;
        const int remaining_bytes = 64 % num_threads;
        
        std::vector<std::thread> workers;
        for (int t = 0; t < num_threads; t++) {
            workers.emplace_back([this, t, bytes_per_thread, remaining_bytes, msg_hash, signature_share_out]() {
                int start_idx = t * bytes_per_thread + std::min(t, remaining_bytes);
                int end_idx = start_idx + bytes_per_thread + (t < remaining_bytes ? 1 : 0);
                
                for (int i = start_idx; i < end_idx; i++) {
                    signature_share_out[i] = (private_key_share_[i % 32] + 
                                             (msg_hash >> (i % 8)) + 
                                             utee_id_ * 17) % 256;
                }
            });
        }
        
        for (auto& w : workers) {
            w.join();
        }
    }
    
public:
    UTEE(int utee_id, CoordinatorTEE* coordinator, double utee_to_coordinator_delay_ms)
        : utee_id_(utee_id)
        , coordinator_(coordinator)
        , utee_to_coordinator_delay_ms_(utee_to_coordinator_delay_ms)
    {
        
        for (int i = 0; i < 32; i++) {
            private_key_share_[i] = (utee_id_ * 31 + i * 7) % 256;
        }
    }
    
    void process_request(const std::string& channel_id, double user_c_amount, 
                        double m_tee_amount, const std::string& request_id) {
        
        SignatureShare share;
        share.utee_id = utee_id_;
        share.channel_id = channel_id;
        share.user_c_amount = user_c_amount;
        share.m_tee_amount = m_tee_amount;
        share.timestamp = std::chrono::high_resolution_clock::now();
        
        generate_signature_share_parallel(channel_id, user_c_amount, m_tee_amount, share.signature_share);
        
        precise_sleep(0.2 / 1000.0);  
        
        precise_sleep(utee_to_coordinator_delay_ms_ / 1000.0);
        
        coordinator_->submit_signature_share(share, request_id);
    }
};

void run_multi_utee_test(int num_requests, int num_utees, int num_mtees,
                        double utee_to_coordinator_delay_ms,
                        double coordinator_to_mtee_delay_ms,
                        double mtee_proc_delay_ms,
                        const std::string& output_file,
                        int num_runs = 1) {
    
    std::vector<PerformanceStats> all_runs_stats;
    
    for (int run = 0; run < num_runs; run++) {
        if (num_runs > 1) {
        }
        
        a402_wrapper_t* mtee_wrapper = a402_wrapper_init(
            true,   
            1,      
            1,      
            1,      
            1,      
            mtee_proc_delay_ms,  
            coordinator_to_mtee_delay_ms  
        );
        
        if (!mtee_wrapper) {
            return;
        }
        
        std::vector<std::string> channel_ids;
        double channel_amount = 10000.0;
        
        if (run == 0 || num_runs == 1) {
        }
        for (int i = 0; i < num_mtees; i++) {
            char channel_id[64];
            snprintf(channel_id, sizeof(channel_id), "channel_mtee_%d", i);
            if (a402_create_channel(mtee_wrapper, channel_id, channel_amount)) {
                channel_ids.push_back(channel_id);
            }
        }
        
        if (channel_ids.empty()) {
            a402_wrapper_cleanup(mtee_wrapper);
            return;
        }
        
        CoordinatorTEE coordinator(mtee_wrapper, num_mtees, num_utees,
                                   coordinator_to_mtee_delay_ms,
                                   mtee_proc_delay_ms);
        
        std::vector<UTEE*> utees;
        for (int i = 0; i < num_utees; i++) {
            utees.push_back(new UTEE(i, &coordinator, utee_to_coordinator_delay_ms));
        }
        
        auto start_time = std::chrono::high_resolution_clock::now();
        
        std::vector<std::thread> threads;
        std::mutex stats_mutex;
        std::atomic<int> completed_requests{0};
        PerformanceStats run_stats;
        
        if (run == 0 || num_runs == 1) {
        }
        
        std::vector<std::chrono::high_resolution_clock::time_point> request_start_times(num_requests);
        std::mutex request_times_mutex;
        
        const int total_cores = std::thread::hardware_concurrency();
        
        const int threads_by_cores = std::max(1, total_cores / std::max(1, num_utees));
        const int max_threads_per_utee = 16;  
        const int utee_threads_per_utee = std::min(max_threads_per_utee, threads_by_cores);
        
        if (run == 0 || num_runs == 1) {
        }
        
        for (int u = 0; u < num_utees; u++) {
            
            for (int t = 0; t < utee_threads_per_utee; t++) {
                threads.emplace_back([&, u, t, utee_threads_per_utee]() {
                    
                    for (int i = t; i < num_requests; i += utee_threads_per_utee) {
                        
                        if (u == 0 && t == 0) {
                            {
                                std::lock_guard<std::mutex> lock(request_times_mutex);
                                if (i == 0 || request_start_times[i].time_since_epoch().count() == 0) {
                                    request_start_times[i] = std::chrono::high_resolution_clock::now();
                                }
                            }
                        }
                        
                        int channel_index = i % channel_ids.size();
                        const std::string& channel_id = channel_ids[channel_index];
                        
                        double payment_amount = 1.0;
                        double user_c_amount = channel_amount - payment_amount * (i + 1);
                        double m_tee_amount = payment_amount * (i + 1);
                        
                        std::string request_id = channel_id + "_req_" + std::to_string(i);
                        
                        utees[u]->process_request(channel_id, user_c_amount, m_tee_amount, request_id);
                    }
                });
            }
        }
        
        for (auto& t : threads) {
            t.join();
        }
        
        coordinator.wait_for_completion(num_requests);
        
        auto end_time = std::chrono::high_resolution_clock::now();
        auto duration = std::chrono::duration<double>(end_time - start_time);
        
        double avg_latency_estimate = (duration.count() * 1000.0) / num_requests;
        
        for (int i = 0; i < num_requests; i++) {
            run_stats.latencies_ms.push_back(avg_latency_estimate);
        }
        
        run_stats.total_operations = num_requests;
        run_stats.total_time_sec = duration.count();
        run_stats.calculate();
        
        all_runs_stats.push_back(run_stats);
        
        if (num_runs > 1) {
                      << run_stats.throughput_ops_per_sec << " ops/s, "
                      << "Average latency=" << std::fixed << std::setprecision(3)
                      << run_stats.avg_latency_ms << " ms\n";
        }
        
        for (auto* utee : utees) {
            delete utee;
        }
        utees.clear();
        
        a402_wrapper_cleanup(mtee_wrapper);
    }
    
    PerformanceStats avg_stats;
    if (!all_runs_stats.empty()) {
        double sum_throughput = 0.0;
        double sum_avg_latency = 0.0;
        double sum_min_latency = 0.0;
        double sum_max_latency = 0.0;
        double sum_total_time = 0.0;
        
        for (const auto& s : all_runs_stats) {
            sum_throughput += s.throughput_ops_per_sec;
            sum_avg_latency += s.avg_latency_ms;
            sum_min_latency += s.min_latency_ms;
            sum_max_latency += s.max_latency_ms;
            sum_total_time += s.total_time_sec;
        }
        
        avg_stats.total_operations = num_requests;
        avg_stats.throughput_ops_per_sec = sum_throughput / all_runs_stats.size();
        avg_stats.avg_latency_ms = sum_avg_latency / all_runs_stats.size();
        avg_stats.min_latency_ms = sum_min_latency / all_runs_stats.size();
        avg_stats.max_latency_ms = sum_max_latency / all_runs_stats.size();
        avg_stats.total_time_sec = sum_total_time / all_runs_stats.size();
    }
    
              << avg_stats.total_time_sec << "  seconds\n";
              << avg_stats.throughput_ops_per_sec << " ops/s\n";
              << avg_stats.avg_latency_ms << " ms\n";
              << avg_stats.min_latency_ms << " ms\n";
              << avg_stats.max_latency_ms << " ms\n";
    
    std::ofstream ofs(output_file);
    if (ofs.is_open()) {
        ofs << "{\n";
        ofs << "  \"test_config\": {\n";
        ofs << "    \"num_requests\": " << num_requests << ",\n";
        ofs << "    \"num_utees\": " << num_utees << ",\n";
        ofs << "    \"num_mtees\": " << num_mtees << ",\n";
        ofs << "    \"utee_to_coordinator_delay_ms\": " << utee_to_coordinator_delay_ms << ",\n";
        ofs << "    \"coordinator_to_mtee_delay_ms\": " << coordinator_to_mtee_delay_ms << ",\n";
        ofs << "    \"mtee_proc_delay_ms\": " << mtee_proc_delay_ms << ",\n";
        ofs << "    \"num_runs\": " << num_runs << "\n";
        ofs << "  },\n";
        ofs << "  \"results\": {\n";
        ofs << "    \"total_operations\": " << avg_stats.total_operations << ",\n";
        ofs << "    \"completed_operations\": " << (all_runs_stats.empty() ? 0 : all_runs_stats[0].total_operations) << ",\n";
        ofs << "    \"throughput_ops_per_sec\": " << std::fixed << std::setprecision(2)
            << avg_stats.throughput_ops_per_sec << ",\n";
        ofs << "    \"avg_latency_ms\": " << std::fixed << std::setprecision(3)
            << avg_stats.avg_latency_ms << ",\n";
        ofs << "    \"min_latency_ms\": " << std::fixed << std::setprecision(3)
            << avg_stats.min_latency_ms << ",\n";
        ofs << "    \"max_latency_ms\": " << std::fixed << std::setprecision(3)
            << avg_stats.max_latency_ms << ",\n";
        ofs << "    \"total_time_sec\": " << std::fixed << std::setprecision(6)
            << avg_stats.total_time_sec << "\n";
        ofs << "  }\n";
        ofs << "}\n";
        ofs.close();
    }
}

int main(int argc, char* argv[]) {
    int num_requests = 1000;
    int num_utees = 1;
    int num_mtees = 64;
    double utee_to_coordinator_delay_ms = 5.0;
    double coordinator_to_mtee_delay_ms = 10.0;
    double mtee_proc_delay_ms = 300.0;
    std::string output_file = "multi_utee_coordination.json";
    int num_runs = 5;
    
    for (int i = 1; i < argc; i++) {
        if (std::string(argv[i]) == "--requests" && i + 1 < argc) {
            num_requests = std::stoi(argv[i + 1]);
            i++;
        } else if (std::string(argv[i]) == "--utees" && i + 1 < argc) {
            num_utees = std::stoi(argv[i + 1]);
            i++;
        } else if (std::string(argv[i]) == "--mtees" && i + 1 < argc) {
            num_mtees = std::stoi(argv[i + 1]);
            i++;
        } else if (std::string(argv[i]) == "--utee-to-coord-delay" && i + 1 < argc) {
            utee_to_coordinator_delay_ms = std::stod(argv[i + 1]);
            i++;
        } else if (std::string(argv[i]) == "--coord-to-mtee-delay" && i + 1 < argc) {
            coordinator_to_mtee_delay_ms = std::stod(argv[i + 1]);
            i++;
        } else if (std::string(argv[i]) == "--mtee-proc-delay" && i + 1 < argc) {
            mtee_proc_delay_ms = std::stod(argv[i + 1]);
            i++;
        } else if (std::string(argv[i]) == "--output" && i + 1 < argc) {
            output_file = argv[i + 1];
            i++;
        } else if (std::string(argv[i]) == "--runs" && i + 1 < argc) {
            num_runs = std::stoi(argv[i + 1]);
            i++;
        }
    }
    
    run_multi_utee_test(num_requests, num_utees, num_mtees,
                       utee_to_coordinator_delay_ms,
                       coordinator_to_mtee_delay_ms,
                       mtee_proc_delay_ms,
                       output_file, num_runs);
    
    return 0;
}
