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
    int total_operations;
    double total_time_sec;
    double throughput_ops_per_sec;
    double avg_latency_ms;
    double min_latency_ms;
    double max_latency_ms;
    std::vector<double> latencies_ms;  
    
    void calculate() {
        if (total_time_sec > 0) {
            throughput_ops_per_sec = total_operations / total_time_sec;
            
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
    }
};

void simulate_utee_to_mtee_communication(double delay_ms) {
    precise_sleep(delay_ms / 1000.0);
}

void run_test(int num_requests, int num_mtees, double comm_delay_ms, 
              double proc_delay_ms, const std::string& output_file, int num_runs = 1) {
    
    std::vector<PerformanceStats> all_runs_stats;
    
    for (int run = 0; run < num_runs; run++) {
        if (num_runs > 1) {
        }
        
        a402_wrapper_t* wrapper = a402_wrapper_init(
            true,   
            1,      
            1,      
            1,      
            1,      
            proc_delay_ms,  
            comm_delay_ms   
        );
        
        if (!wrapper) {
            return;
        }
        
        std::vector<std::string> channel_ids;
        double channel_amount = 10000.0;  
        
        if (run == 0 || num_runs == 1) {
        }
        for (int i = 0; i < num_mtees; i++) {
            char channel_id[64];
            snprintf(channel_id, sizeof(channel_id), "channel_mtee_%d", i);
            if (a402_create_channel(wrapper, channel_id, channel_amount)) {
                channel_ids.push_back(channel_id);
            } else {
            }
        }
        
        if (channel_ids.empty()) {
            a402_wrapper_cleanup(wrapper);
            return;
        }
        
        if (run == 0 || num_runs == 1) {
        }
        
        PerformanceStats run_stats;
        run_stats.latencies_ms.clear();
        
        auto start_time = std::chrono::high_resolution_clock::now();
        
        std::vector<std::thread> threads;
        std::mutex stats_mutex;
        std::atomic<int> completed_requests{0};
    
        int requests_per_mtee = num_requests / num_mtees;
        int remaining_requests = num_requests % num_mtees;
        
        if (run == 0 || num_runs == 1) {
        }
    
    for (int m = 0; m < num_mtees && m < (int)channel_ids.size(); m++) {
        threads.emplace_back([&, m]() {
            int start_idx = m * requests_per_mtee + std::min(m, remaining_requests);
            int end_idx = start_idx + requests_per_mtee + (m < remaining_requests ? 1 : 0);
            
            const std::string& channel_id = channel_ids[m];
            
            int num_sub_threads = std::min(100, end_idx - start_idx);  
            if (num_sub_threads <= 0) num_sub_threads = 1;
            
            std::vector<std::thread> sub_threads;
            int requests_per_sub_thread = (end_idx - start_idx) / num_sub_threads;
            int remaining_sub_requests = (end_idx - start_idx) % num_sub_threads;
            
            for (int t = 0; t < num_sub_threads; t++) {
                sub_threads.emplace_back([&, t, start_idx, end_idx, channel_id]() {
                    int sub_start = start_idx + t * requests_per_sub_thread + std::min(t, remaining_sub_requests);
                    int sub_end = sub_start + requests_per_sub_thread + (t < remaining_sub_requests ? 1 : 0);
                    
                    for (int i = sub_start; i < sub_end; i++) {
                        
                        auto req_start = std::chrono::high_resolution_clock::now();
                        
                        double payment_amount = 1.0;
                        double user_c_amount = channel_amount - payment_amount * (i - start_idx + 1);
                        double m_tee_amount = payment_amount * (i - start_idx + 1);
                        
                        bool success = a402_update_channel(
                            wrapper,
                            channel_id.c_str(),
                            user_c_amount,
                            m_tee_amount
                        );
                        
                        auto req_end = std::chrono::high_resolution_clock::now();
                        double req_latency_ms = std::chrono::duration<double, std::milli>(req_end - req_start).count();
                        
                        if (success) {
                            completed_requests++;
                            
                            {
                                std::lock_guard<std::mutex> lock(stats_mutex);
                                run_stats.latencies_ms.push_back(req_latency_ms);
                            }
                        }
                    }
                });
            }
            
            for (auto& st : sub_threads) {
                st.join();
            }
        });
    }
    
        for (auto& t : threads) {
            t.join();
        }
        
        auto end_time = std::chrono::high_resolution_clock::now();
        auto duration = std::chrono::duration<double>(end_time - start_time);
        
        run_stats.total_operations = num_requests;
        run_stats.total_time_sec = duration.count();
        run_stats.calculate();
        
        all_runs_stats.push_back(run_stats);
        
        if (num_runs > 1) {
                      << run_stats.throughput_ops_per_sec << " ops/s, "
                      << "Average latency=" << std::fixed << std::setprecision(3) 
                      << run_stats.avg_latency_ms << " ms\n";
        }
        
        a402_wrapper_cleanup(wrapper);
    }
    
    PerformanceStats avg_stats;
    if (!all_runs_stats.empty()) {
        double sum_throughput = 0.0;
        double sum_avg_latency = 0.0;
        double sum_min_latency = 0.0;
        double sum_max_latency = 0.0;
        double sum_total_time = 0.0;
        int total_completed = 0;
        
        for (const auto& s : all_runs_stats) {
            sum_throughput += s.throughput_ops_per_sec;
            sum_avg_latency += s.avg_latency_ms;
            sum_min_latency += s.min_latency_ms;
            sum_max_latency += s.max_latency_ms;
            sum_total_time += s.total_time_sec;
            total_completed += s.total_operations;
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
    if (num_runs > 1) {
        for (size_t i = 0; i < all_runs_stats.size(); i++) {
                      << "Throughput=" << std::fixed << std::setprecision(2) 
                      << all_runs_stats[i].throughput_ops_per_sec << " ops/s, "
                      << "Average latency=" << std::fixed << std::setprecision(3) 
                      << all_runs_stats[i].avg_latency_ms << " ms\n";
        }
    }
    
    double single_request_time_ms = comm_delay_ms * 2 + proc_delay_ms;
    double theoretical_time_sec = (single_request_time_ms / 1000.0) * (num_requests / num_mtees);
    double theoretical_throughput = num_requests / theoretical_time_sec;
    
              << proc_delay_ms << " ms)\n";
              << "  requests): " << std::fixed << std::setprecision(3) 
              << theoretical_time_sec << "  seconds\n";
              << theoretical_throughput << " ops/s\n";
    
    std::ofstream ofs(output_file);
    if (ofs.is_open()) {
        ofs << "{\n";
        ofs << "  \"test_config\": {\n";
        ofs << "    \"num_requests\": " << num_requests << ",\n";
        ofs << "    \"num_mtees\": " << num_mtees << ",\n";
        ofs << "    \"communication_delay_ms\": " << comm_delay_ms << ",\n";
        ofs << "    \"processing_delay_ms\": " << proc_delay_ms << "\n";
        ofs << "  },\n";
        ofs << "  \"results\": {\n";
        ofs << "    \"total_operations\": " << avg_stats.total_operations << ",\n";
        ofs << "    \"completed_operations\": " << (all_runs_stats.empty() ? 0 : all_runs_stats[0].total_operations) << ",\n";
        ofs << "    \"num_runs\": " << num_runs << ",\n";
        ofs << "    \"throughput_ops_per_sec\": " << std::fixed << std::setprecision(2) 
            << avg_stats.throughput_ops_per_sec << ",\n";
        ofs << "    \"avg_latency_ms\": " << std::fixed << std::setprecision(3) 
            << avg_stats.avg_latency_ms << ",\n";
        ofs << "    \"min_latency_ms\": " << std::fixed << std::setprecision(3) 
            << avg_stats.min_latency_ms << ",\n";
        ofs << "    \"max_latency_ms\": " << std::fixed << std::setprecision(3) 
            << avg_stats.max_latency_ms << ",\n";
        ofs << "    \"total_time_sec\": " << std::fixed << std::setprecision(6) 
            << avg_stats.total_time_sec << ",\n";
        ofs << "    \"runs\": [\n";
        for (size_t i = 0; i < all_runs_stats.size(); i++) {
            ofs << "      {\n";
            ofs << "        \"run\": " << (i + 1) << ",\n";
            ofs << "        \"throughput_ops_per_sec\": " << std::fixed << std::setprecision(2) 
                << all_runs_stats[i].throughput_ops_per_sec << ",\n";
            ofs << "        \"avg_latency_ms\": " << std::fixed << std::setprecision(3) 
                << all_runs_stats[i].avg_latency_ms << ",\n";
            ofs << "        \"total_time_sec\": " << std::fixed << std::setprecision(6) 
                << all_runs_stats[i].total_time_sec << "\n";
            ofs << "      }";
            if (i < all_runs_stats.size() - 1) ofs << ",";
            ofs << "\n";
        }
        ofs << "    ]\n";
        ofs << "  },\n";
        ofs << "  \"theoretical\": {\n";
        ofs << "    \"single_request_time_ms\": " << single_request_time_ms << ",\n";
        ofs << "    \"theoretical_time_sec\": " << std::fixed << std::setprecision(6) 
            << theoretical_time_sec << ",\n";
        ofs << "    \"theoretical_throughput_ops_per_sec\": " << std::fixed << std::setprecision(2) 
            << theoretical_throughput << "\n";
        ofs << "  }\n";
        ofs << "}\n";
        ofs.close();
    }
    
}

int main(int argc, char* argv[]) {
    int num_requests = 1000;
    int num_mtees = 64;  
    double comm_delay_ms = 10.0;
    double proc_delay_ms = 300.0;
    std::string output_file = "utee_mtee_distribution.json";
    int num_runs = 1;  
    
    for (int i = 1; i < argc; i++) {
        if (std::string(argv[i]) == "--requests" && i + 1 < argc) {
            num_requests = std::stoi(argv[i + 1]);
            i++;
        } else if (std::string(argv[i]) == "--mtees" && i + 1 < argc) {
            num_mtees = std::stoi(argv[i + 1]);
            i++;
        } else if (std::string(argv[i]) == "--comm-delay" && i + 1 < argc) {
            comm_delay_ms = std::stod(argv[i + 1]);
            i++;
        } else if (std::string(argv[i]) == "--proc-delay" && i + 1 < argc) {
            proc_delay_ms = std::stod(argv[i + 1]);
            i++;
        } else if (std::string(argv[i]) == "--output" && i + 1 < argc) {
            output_file = argv[i + 1];
            i++;
        } else if (std::string(argv[i]) == "--runs" && i + 1 < argc) {
            num_runs = std::stoi(argv[i + 1]);
            i++;
        }
    }
    
    run_test(num_requests, num_mtees, comm_delay_ms, proc_delay_ms, output_file, num_runs);
    
    return 0;
}
