#ifndef LOGGER_H
#define LOGGER_H

#include <stdint.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif


typedef enum {
    LOG_DEBUG = 0,
    LOG_INFO = 1,
    LOG_WARN = 2,
    LOG_ERROR = 3
} log_level_t;


void logger_init(log_level_t level);


void logger_set_level(log_level_t level);


void logger_log(log_level_t level, const char* file, int line, const char* format, ...);


#define LOG_DEBUG(...) logger_log(LOG_DEBUG, __FILE__, __LINE__, __VA_ARGS__)
#define LOG_INFO(...) logger_log(LOG_INFO, __FILE__, __LINE__, __VA_ARGS__)
#define LOG_WARN(...) logger_log(LOG_WARN, __FILE__, __LINE__, __VA_ARGS__)
#define LOG_ERROR(...) logger_log(LOG_ERROR, __FILE__, __LINE__, __VA_ARGS__)

#ifdef __cplusplus
}
#endif

#endif 


