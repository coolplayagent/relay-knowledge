const C_DRIVER_OPS_H: &str = r#"#ifndef RK_DRIVER_OPS_H
#define RK_DRIVER_OPS_H

#include <stddef.h>

struct rk_device;

typedef int (*rk_open_fn)(struct rk_device *dev);
typedef int (*rk_read_fn)(struct rk_device *dev, char *buffer, size_t length);

struct rk_driver_ops {
    rk_open_fn open;
    rk_read_fn read;
    void (*close)(struct rk_device *dev);
};

// RK_HEADER_CONTRACT_NOTE keeps callback ownership searchable when parsers miss comments.
int rk_driver_open(struct rk_device *dev);
int rk_driver_read(struct rk_device *dev, char *buffer, size_t length);
void rk_driver_close(struct rk_device *dev);
int rk_dispatch_read(
    const struct rk_driver_ops *ops,
    struct rk_device *dev,
    char *buffer,
    size_t length);

#endif
"#;

const C_MACROS_H: &str = r#"#ifndef RK_MACROS_H
#define RK_MACROS_H

#define RK_STATUS_CLOSED 0
#define RK_STATUS_READY 1
#define RK_TRACE_VALUE(value) ((value) + 17)
#define RK_TOKEN_PASTE(left, right) left##right
#define RK_DECLARE_HANDLER(name) int name(struct rk_device *dev)

enum rk_stage {
    RK_STAGE_VALIDATE = 0,
    RK_STAGE_LOCK = 1,
    RK_STAGE_READ = 2,
};

#define RK_STAGE_ROW(name) [RK_STAGE_##name] = #name

#endif
"#;

const C_DRIVER_OPS_C: &str = r#"#include "driver_ops.h"
#include "macros.h"

struct rk_device {
    int fd;
    int state;
};

int rk_driver_open(struct rk_device *dev)
{
    dev->state = RK_STATUS_READY;
    return dev->state;
}

int rk_driver_read(struct rk_device *dev, char *buffer, size_t length)
{
    // RK_TRACE_NOTE documents fallback-only macro text.
    buffer[0] = (char)RK_TRACE_VALUE(dev->fd);
    return (int)length;
}

void rk_driver_close(struct rk_device *dev)
{
    dev->state = RK_STATUS_CLOSED;
}

const struct rk_driver_ops rk_default_ops = {
    .open = rk_driver_open,
    .read = rk_driver_read,
    .close = rk_driver_close,
};
"#;

const C_DISPATCH_C: &str = r#"#include "driver_ops.h"

static int rk_validate_device(struct rk_device *dev)
{
    return dev != 0;
}

static int rk_lock_device(struct rk_device *dev)
{
    return dev != 0;
}

static void rk_unlock_device(struct rk_device *dev)
{
    (void)dev;
}

typedef int (*rk_stage_fn)(struct rk_device *dev);

static rk_stage_fn rk_pipeline[] = {
    rk_validate_device,
    rk_lock_device,
};

int rk_dispatch_read(
    const struct rk_driver_ops *ops,
    struct rk_device *dev,
    char *buffer,
    size_t length)
{
    if (!rk_validate_device(dev)) {
        return -1;
    }
    if (ops->open(dev) < 0) {
        return -1;
    }
    if (rk_lock_device(dev) < 0) {
        return -1;
    }
    int result = ops->read(dev, buffer, length);
    rk_unlock_device(dev);
    return result;
}

int rk_run_pipeline(struct rk_device *dev)
{
    int total = 0;
    for (unsigned int index = 0; index < 2; ++index) {
        total += rk_pipeline[index](dev);
    }
    return total;
}

// RK_PIPELINE_NOTE records dispatch ordering for exact source-text fallback.
"#;

const C_GENERATED_TABLE_C: &str = r#"#include "driver_ops.h"
#include "macros.h"

struct rk_table_row {
    const char *name;
    rk_read_fn read;
};

static const char *rk_stage_names[] = {
    RK_STAGE_ROW(VALIDATE),
    RK_STAGE_ROW(LOCK),
    RK_STAGE_ROW(READ),
};

// RK_STAGE_TABLE_NOTE documents generated stage rows for grep fallback recall.
static const struct rk_table_row rk_rows[] = {
    [RK_STAGE_READ] = {
        .name = "read",
        .read = rk_driver_read,
    },
};

int rk_table_read(struct rk_device *dev, char *buffer, size_t length)
{
    (void)rk_stage_names;
    return rk_rows[RK_STAGE_READ].read(dev, buffer, length);
}
"#;

const C_HTTP_MACRO_MODULE_C: &str = r#"#include "driver_ops.h"
#include <openssl/ssl.h>

#define RK_HTTP_HANDLER(name) int name(struct rk_device *dev)
#define RK_HTTP_MODULE_ENTRY(name) { #name, name }

struct rk_http_module_entry {
    const char *name;
    int (*handler)(struct rk_device *dev);
};

RK_HTTP_HANDLER(rk_http_access_handler)
{
    return dev != 0;
}

static const struct rk_http_module_entry rk_http_modules[] = {
    RK_HTTP_MODULE_ENTRY(rk_http_access_handler),
};
"#;

const C_NGINX_EXTERNAL_MODULE_C: &str = r#"#include <ngx_config.h>
#include <ngx_core.h>
#include <ngx_http.h>

#define KONG_ACCESS_PHASE(name) static ngx_int_t name(ngx_http_request_t *request)

static ngx_int_t
ngx_http_demo_init(ngx_pool_t *pool)
{
    return ngx_array_init(pool);
}

KONG_ACCESS_PHASE(ngx_http_demo_access)
{
    return ngx_http_demo_init(request->pool);
}

static ngx_command_t ngx_http_demo_commands[] = {
    { ngx_string("demo"), NGX_HTTP_LOC_CONF, ngx_conf_set_str_slot, 0, 0, NULL },
    ngx_null_command
};

static ngx_http_module_t ngx_http_demo_module_ctx = {
    NULL,
    ngx_http_demo_init,
    NULL,
    NULL
};

ngx_module_t ngx_http_demo_module = {
    NGX_MODULE_V1,
    &ngx_http_demo_module_ctx,
    ngx_http_demo_commands,
    NGX_HTTP_MODULE,
    NULL,
    NULL,
    NULL,
    NULL,
    NULL,
    NULL,
    NULL,
    NGX_MODULE_V1_PADDING
};
"#;

const C_GCC_EXTENSION_POLICY_C: &str = r#"#include "securec.h"

#define WILD_MULTI_CHAR '*'

typedef struct PdpString {
    const char *data;
} PdpString;

typedef struct PdpStack PdpStack;

typedef struct PdpPolicyEntry {
    const char *name;
    int (*match)(PdpStack *stack, PdpString *pattern);
} PdpPolicyEntry;

static attribute((always_inline)) int pdp_wildcard_step(PdpString *pattern, int index)
{
    return pattern->data[index] == WILD_MULTI_CHAR;
}

static __attribute__((always_inline)) int pdp_secure_copy(PdpString *target, const PdpString *source)
{
    return memcpy_s((void *)target->data, 16, source->data, 16);
}

__always_inline int pdp_policy_regex_match(PdpStack *stack, PdpString *pattern)
{
    (void)stack;
    return pdp_wildcard_step(pattern, 0) || pdp_secure_copy(pattern, pattern);
}

static PdpPolicyEntry pdp_policy_entries[] = {
    { "wildcard", pdp_policy_regex_match },
};
"#;

const C_FAKE_DRIVER_C: &str = r#"#include "driver_ops.h"

int rk_driver_read_fake(struct rk_device *dev, char *buffer, size_t length)
{
    (void)dev;
    (void)buffer;
    return (int)length;
}
"#;

const CPP_CACHE_HPP: &str = r#"#pragma once

#include <memory>
#include <string>
#include <vector>

namespace rk::store {

class Writer {
 public:
    virtual ~Writer() = default;
    virtual void Append(const std::string& key) = 0;
};

template <typename Key>
class Cache {
 public:
    using KeyList = std::vector<Key>;

    explicit Cache(std::unique_ptr<Writer> writer);
    void Insert(const Key& key);
    const Key& Lookup(const Key& key) const;

 private:
    std::unique_ptr<Writer> writer_;
    KeyList keys_;
};

class RecordingWriter final : public Writer {
 public:
    void Append(const std::string& key) override;
};

}  // namespace rk::store
"#;

const CPP_EXPORTED_MODULE_HPP: &str = r#"#pragma once

#include <boost/asio.hpp>
#include <string>

#define RK_STORE_API __attribute__((visibility("default")))

namespace rk::store {

class BaseModule {
 public:
    virtual ~BaseModule() = default;
};

RK_STORE_API class HttpModule final : public BaseModule {
 public:
    void Start(const std::string& route);
};

}  // namespace rk::store
"#;

const CPP_PIPELINE_HPP: &str = r#"#pragma once

#include "store/cache.hpp"

#include <memory>
#include <string>
#include <vector>

namespace rk::store {

struct PipelineEvent {
    std::string key;
};

class Pipeline {
 public:
    int operator()(const PipelineEvent& event) const;
};

std::unique_ptr<Cache<std::string>> BuildCache(std::unique_ptr<Writer> writer);
int RunPipeline(Cache<std::string>& cache, const std::vector<PipelineEvent>& events);

}  // namespace rk::store
"#;

const CPP_CACHE_CPP: &str = r#"#include "store/cache.hpp"

#include <utility>

namespace rk::store {

template <typename Key>
Cache<Key>::Cache(std::unique_ptr<Writer> writer) : writer_(std::move(writer)) {}

template <typename Key>
void Cache<Key>::Insert(const Key& key)
{
    keys_.push_back(key);
    writer_->Append(std::string(key));
}

template <typename Key>
const Key& Cache<Key>::Lookup(const Key& key) const
{
    for (const auto& candidate : keys_) {
        if (candidate == key) {
            return candidate;
        }
    }
    return keys_.front();
}

void RecordingWriter::Append(const std::string& key)
{
    (void)key;
}

template class Cache<std::string>;

}  // namespace rk::store
"#;

const CPP_PIPELINE_CPP: &str = r#"#include "store/pipeline.hpp"

#include <utility>

namespace rk::store {

namespace cache_alias = rk::store;

std::unique_ptr<Cache<std::string>> BuildCache(std::unique_ptr<Writer> writer)
{
    return std::make_unique<cache_alias::Cache<std::string>>(std::move(writer));
}

int Pipeline::operator()(const PipelineEvent& event) const
{
    return static_cast<int>(event.key.size());
}

int RunPipeline(Cache<std::string>& cache, const std::vector<PipelineEvent>& events)
{
    Pipeline pipeline;
    auto append_event = [&cache, &pipeline](const PipelineEvent& event) {
        cache.Insert(event.key);
        return pipeline(event);
    };
    int total = 0;
    for (const auto& event : events) {
        total += append_event(event);
    }
    return total;
}

}  // namespace rk::store
"#;

const CPP_FAKE_CACHE_CPP: &str = r#"#include "store/cache.hpp"

namespace rk::store::test {

class FakeCache {
 public:
    void Insert(const std::string& key)
    {
        (void)key;
    }
};

}  // namespace rk::store::test
"#;
