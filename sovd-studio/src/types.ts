import { z } from "zod";

export const EcuSchema = z.object({
  id: z.string(),
  name: z.string(),
  domain: z.string(),
  status: z.string(),
});
export type Ecu = z.infer<typeof EcuSchema>;

export const ValueSchema = z.object({
  name: z.string(),
  value: z.number(),
  unit: z.string(),
});
export type DiagnosticValue = z.infer<typeof ValueSchema>;

export const FaultSchema = z.object({
  code: z.string(),
  description: z.string(),
  active: z.boolean(),
});
export type Fault = z.infer<typeof FaultSchema>;

export const OperationSchema = z.object({
  operation: z.string(),
  success: z.boolean(),
  message: z.string(),
});


export const UdsResponseSchema = z.object({
  ecu_id: z.string(),
  request_hex: z.string(),
  response_hex: z.string().nullable(),
});
export type UdsResponse = z.infer<typeof UdsResponseSchema>;

export const TopologySchema = z.object({
  vehicle: z.object({ id: z.string(), name: z.string() }),
  central: z.object({
    namespace: z.string(),
    sovd_bind: z.string(),
    sovd_port: z.number(),
  }),
  zones: z.array(z.object({
    id: z.string(),
    name: z.string(),
    namespace: z.string(),
    can_interface: z.string(),
    central_ip: z.string(),
    zonal_ip: z.string(),
    doip_port: z.number(),
    doip_logical_address: z.number(),
  })),
  ecus: z.array(z.object({
    id: z.string(),
    name: z.string(),
    domain: z.string(),
    zone: z.string(),
    doip_logical_address: z.number(),
    can_interface: z.string(),
    can_request_id: z.number(),
    can_response_id: z.number(),
  })),
});
export type VehicleTopology = z.infer<typeof TopologySchema>;

export const HealthSchema = z.object({
  vehicle_id: z.string(),
  vehicle_name: z.string(),
  overall: z.string(),
  zones: z.array(z.object({
    id: z.string(),
    name: z.string(),
    online: z.boolean(),
    detail: z.string(),
  })),
});
export type VehicleHealth = z.infer<typeof HealthSchema>;


// Server-discovered SOVD demonstrator resources.
export const ResourceDescriptorSchema = z.object({
  id: z.string(),
  path: z.string(),
  kind: z.string(),
  description: z.string(),
  methods: z.array(z.string()),
  simulated: z.boolean(),
});
export type ResourceDescriptor = z.infer<typeof ResourceDescriptorSchema>;

export const OperationDescriptorSchema = z.object({
  id: z.string(),
  name: z.string(),
  method: z.string(),
  execution_path: z.string(),
  description: z.string(),
  supports_execution: z.boolean(),
});
export type OperationDescriptor = z.infer<typeof OperationDescriptorSchema>;

export const OperationExecutionSchema = z.object({
  execution_id: z.string(),
  ecu_id: z.string(),
  operation: z.string(),
  status: z.string(),
  success: z.boolean(),
  message: z.string(),
  created_at_unix_ms: z.number(),
});
export type OperationExecution = z.infer<typeof OperationExecutionSchema>;
