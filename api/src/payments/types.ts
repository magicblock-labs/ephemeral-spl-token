export type PaymentCluster = "mainnet" | "devnet" | "mainnet-private" | "devnet-private";

export type PaymentTerms = {
  id: string;
  merchantId: string;
  payer: string;
  recipient: string;
  amount: string;
  mint: string;
  cluster: PaymentCluster;
  externalReference: string;
  resource: string;
  description?: string;
  requestHash?: string;
  expiresAt: string;
};

export type PreparedPayment = {
  transactionBase64: string;
  messageBase64: string;
  recentBlockhash: string;
  lastValidBlockHeight: number;
  validator: string;
  // Server configuration only. Never accept an RPC URL from a payment payload.
  rpcEndpoint: string;
};

export type PaymentStatus = "created" | "pending" | "paid" | "failed" | "expired";

export type PaymentRecord = PaymentTerms & {
  accessTokenHash: string;
  status: PaymentStatus;
  createdAt: string;
  prepared?: PreparedPayment;
  signedTransactionBase64?: string;
  signature?: string;
  confirmedAt?: string;
  slot?: number;
  failure?: string;
  reconcileAttempts?: number;
  blockhashExpired?: boolean;
};

export type PaymentView = PaymentTerms & {
  status: PaymentStatus;
  settlement: "ephemeral-rollup";
  signature?: string;
  confirmedAt?: string;
  slot?: number;
  failure?: string;
};

export type MerchantRecord = {
  id: string;
  wallet: string;
  apiKeyHash: string;
  createdAt: string;
};
