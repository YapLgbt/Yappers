#ifndef YAPPERS_NETKIT_H
#define YAPPERS_NETKIT_H
#include <stdint.h>
/* ABI v1: all returned strings are owned allocations. Free each exactly once.
 * Call open/request/close on a worker thread. Inputs are borrowed UTF-8 JSON,
 * NUL-terminated, and valid only for the duration of the call. Never send NULs.
 * Every result is {"ok":true,"data":...} or {"ok":false,"error":"..."}.
 * Handles are opaque registry IDs, not pointers. close is idempotent.
 */
char *yappers_netkit_identity(void);
/* Offline portable account/archive operations; same owned-string contract. */
char *yappers_netkit_portability(const char *command_json);
char *yappers_netkit_open(const char *config_json);
char *yappers_netkit_request(uint64_t handle, const char *command_json);
/* End is idempotent and interrupts in-flight requests belonging to this refresh only. */
uint64_t yappers_netkit_begin_refresh(void);
void yappers_netkit_end_refresh(uint64_t scope);
char *yappers_netkit_refresh_request(uint64_t scope, uint64_t handle, const char *command_json);
char *yappers_netkit_close(uint64_t handle);
void yappers_netkit_string_free(char *owned_string);
#endif
