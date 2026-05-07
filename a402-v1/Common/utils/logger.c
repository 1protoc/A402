#include "logger.h"
#include <stdio.h>
#include <stdarg.h>
#include <time.h>
#include <string.h>

static log_level_t g_log_level = LOG_INFO;

void logger_init(log_level_t level) {
    g_log_level = level;
}

void logger_set_level(log_level_t level) {
    g_log_level = level;
}

void logger_log(log_level_t level, const char* file, int line, const char* format, ...) {
    if (level < g_log_level) {
        return;
    }
    
    const char* level_str[] = {"DEBUG", "INFO", "WARN", "ERROR"};
    const char* level_color[] = {"\033[36m", "\033[32m", "\033[33m", "\033[31m"};
    const char* reset_color = "\033[0m";
    
    time_t now = time(NULL);
    struct tm* tm_info = localtime(&now);
    char time_str[64];
    strftime(time_str, sizeof(time_str), "%Y-%m-%d %H:%M:%S", tm_info);
    
    
    const char* filename = strrchr(file, '/');
    if (filename) {
        filename++;
    } else {
        filename = file;
    }
    
    printf("%s[%s]%s %s:%d: ", 
           level_color[level], level_str[level], reset_color,
           filename, line);
    
    va_list args;
    va_start(args, format);
    vprintf(format, args);
    va_end(args);
    
    printf("\n");
    fflush(stdout);
}


