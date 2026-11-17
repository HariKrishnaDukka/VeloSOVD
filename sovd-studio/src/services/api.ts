import { z } from "zod";
import {
  EcuSchema, ValueSchema, FaultSchema, OperationSchema,
  UdsResponseSchema, TopologySchema, HealthSchema,
  ResourceDescriptorSchema, OperationDescriptorSchema,
  OperationExecutionSchema,
} from "../types";

async function get<T>(path: string, schema: z.ZodType<T>): Promise<T> {
  const response = await fetch(path, { headers: { Accept: "application/json" } });
  if (!response.ok) throw new Error(`HTTP ${response.status}: ${response.statusText}`);
  return schema.parse(await response.json());
}

export const api = {
  ecus: () => get("/api/ecus", z.array(EcuSchema)),
  topology: () => get("/api/topology", TopologySchema),
  health: () => get("/api/health", HealthSchema),
  udsRequest: async (id: string, payloadHex: string) => {
    const response = await fetch(
      `/api/ecus/${encodeURIComponent(id)}/diagnostics/requests`,
      {
        method: "POST",
        headers: { "Content-Type": "application/json", Accept: "application/json" },
        body: JSON.stringify({ payload_hex: payloadHex }),
      },
    );
    if (!response.ok) {
      throw new Error(`HTTP ${response.status}: ${response.statusText}`);
    }
    return UdsResponseSchema.parse(await response.json());
  },
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
    const response = await fetch(executionPath, {
      method: "POST",
      headers: { Accept: "application/json" },
    });
    if (!response.ok) {
      throw new Error(`HTTP ${response.status}: ${response.statusText}`);
    }
    return OperationExecutionSchema.parse(await response.json());
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
  clearFaults: async (id: string) => {
    const response = await fetch(
      `/api/ecus/${encodeURIComponent(id)}/diagnostics/operations/clear-faults`,
      { method: "POST", headers: { Accept: "application/json" } },
    );
    if (!response.ok) throw new Error(`HTTP ${response.status}: ${response.statusText}`);
    return OperationSchema.parse(await response.json());
  },
};
