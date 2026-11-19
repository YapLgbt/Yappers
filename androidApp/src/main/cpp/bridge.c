#include <jni.h>
#include <limits.h>
#include <stdlib.h>
#include <string.h>
#include "netkit.h"

void yappers_netkit_android_initialize(void *vm, void *application);

JNIEXPORT void JNICALL Java_app_yappers_AndroidAbi_initialize(JNIEnv *env, jobject self, jobject app) {
    JavaVM *vm;
    if ((*env)->GetJavaVM(env, &vm) != JNI_OK) return;
    // Iroh borrows both arguments for the process lifetime.
    jobject global = (*env)->NewGlobalRef(env, app);
    if (global) yappers_netkit_android_initialize(vm, global);
}

JNIEXPORT jlong JNICALL Java_app_yappers_AndroidAbi_beginRefresh(JNIEnv *env, jobject self) {
    return (jlong)yappers_netkit_begin_refresh();
}
JNIEXPORT void JNICALL Java_app_yappers_AndroidAbi_endRefresh(JNIEnv *env, jobject self, jlong token) {
    yappers_netkit_end_refresh((uint64_t)token);
}

// Byte arrays carry ordinary UTF-8, including emoji; JNI's modified UTF-8 strings do not.
JNIEXPORT jbyteArray JNICALL Java_app_yappers_AndroidAbi_call(JNIEnv *env, jobject self,
        jint op, jlong handle, jlong token, jbyteArray input) {
    char *arg = NULL;
    char *result = NULL;
    if (input != NULL) {
        jsize len = (*env)->GetArrayLength(env, input);
        arg = malloc((size_t)len + 1);
        if (!arg) return NULL;
        (*env)->GetByteArrayRegion(env, input, 0, len, (jbyte *)arg);
        if ((*env)->ExceptionCheck(env)) { free(arg); return NULL; }
        arg[len] = 0;
    }
    switch (op) {
        case 0: result = yappers_netkit_identity(); break;
        case 1: result = yappers_netkit_portability(arg); break;
        case 2: result = yappers_netkit_open(arg); break;
        case 3:
            result = token ? yappers_netkit_refresh_request(token, handle, arg)
                           : yappers_netkit_request(handle, arg);
            break;
        case 4: result = yappers_netkit_close(handle); break;
    }
    free(arg);
    if (!result) return NULL;
    size_t len = strlen(result);
    jbyteArray bytes = len <= INT_MAX ? (*env)->NewByteArray(env, (jsize)len) : NULL;
    if (bytes) (*env)->SetByteArrayRegion(env, bytes, 0, (jsize)len, (const jbyte *)result);
    yappers_netkit_string_free(result);
    return bytes;
}
