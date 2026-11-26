import { z } from "zod";
import { ensureOk, request } from "./transport";
import {
  EcuSchema, ValueSchema, FaultSchema, OperationSchema,
  UdsResponseSchema, TopologySchema, HealthSchema,
  ResourceDescriptorSchema, OperationDescriptorSchema,
  OperationExecutionSchema,
} from "../types";

async function call<T>(method: string, path: string, schema: z.ZodType<T>, body?: unknown): Promise<T> {
  const response = ensureOk(await request(method, path, body === undefined ? undefined : JSON.stringify(body)));
  return schema.parse(response.body ? JSON.parse(response.body) : null);
}

const get = <T,>(path: string, schema: z.ZodType<T>) => call("GET", path, schema);

export const api = {
  ecus: () => get("/api/ecus", z.array(EcuSchema)),
  topology: () => get("/api/topology", TopologySchema),
  health: () => get("/api/health", HealthSchema),
  udsRequest: (id: string, payloadHex: string) =>
    call(
      "POST",
      `/api/ecus/${encodeURIComponent(id)}/diagnostics/requests`,
      UdsResponseSchema,
      { payload_hex: payloadHex },
    ),
  speed: (id: string) =>
    get(`/api/ecus/${encodeURIComponent(id)}/data/vehicle-speed`, ValueSchema),
  resources: (id: string) =>
    get(`/api/ecus/${encodeURIComponent(id)}/resources`, z.array(ResourceDescriptorSchema)),
  operations: (id: string) =>
    get(`/api/ecus/${encodeURIComponent(id)}/operations`, z.array(OperationDescriptorSchema)),
  faults: (id: string, active?: boolean) => {
    const query = active === undefined ? "" : `?active=${String(active)}`;
    return get(
      `/api/ecus/${encodeURIComponent(id)}/diagnostics/faults${query}`,
      z.array(FaultSchema),
    );
  },
  executeOperation: async (executionPath: string) => {
    if (!executionPath.startsWith("/api/") || executionPath.startsWith("//")) {
      throw new Error("Invalid operation execution path returned by server.");
    }
    return call("POST", executionPath, OperationExecutionSchema);
  },
  executions: (id: string, operationId: string) =>
  get(
    `/api/ecus/${encodeURIComponent(id)}/operations/${encodeURIComponent(operationId)}/executions`,
    z.array(OperationExecutionSchema),
  ),
execution: (id: string, operationId: string, executionId: string) =>
  get(
    `/api/ecus/${encodeURIComponent(id)}/operations/${encodeURIComponent(operationId)}/executions/${encodeURIComponent(executionId)}`,
    OperationExecutionSchema,
  ),
  clearFaults: (id: string) =>
    call(
      "POST",
      `/api/ecus/${encodeURIComponent(id)}/diagnostics/operations/clear-faults`,
      OperationSchema,
    ),
};
