import { AppEnv } from "../env";
import { ApiError } from "./errors";
import { resolveRpcConfig } from "./solana";

export type ChallengeInput = {
    pubkey: string;
    cluster?: string;
};

export type ChallengeResponse = {
    challenge: string;
};

export type LoginInput = {
    pubkey: string;
    challenge: string;
    signature: string;
    cluster?: string;
};

export type LoginResponse = {
    token: string;
};

export type AuthChallengeResponse = {
    challenge: string;
    error?: string;
};

export function parseAuthToken(headers: Record<string, string>): string | undefined {
    const authToken = headers["Authorization"] ?? headers["authorization"];
    if (!authToken) {
        return undefined;
    }
    return authToken.split(" ")[1];
}

export async function getChallenge(env: AppEnv, input: ChallengeInput): Promise<ChallengeResponse> {
    const config = resolveRpcConfig(env, input.cluster);
    const challengeResponse = await fetch(
        `${config.ephemeralRpcUrl}/auth/challenge?pubkey=${input.pubkey}`,
    );

    if (!challengeResponse.ok) {
        throw new ApiError(challengeResponse.status, "RPC_ERROR", `Failed to get challenge: ${challengeResponse.statusText}`);
    }

    const { challenge, error }: AuthChallengeResponse =
        await challengeResponse.json();

    if (typeof error === "string" && error.length > 0) {
        throw new ApiError(502, "RPC_ERROR", `Failed to get challenge: ${error}`);
    }
    if (typeof challenge !== "string" || challenge.length === 0) {
        throw new ApiError(502, "RPC_ERROR", "No challenge received");
    }

    return {
        challenge,
    };
}

export async function login(env: AppEnv, input: LoginInput): Promise<LoginResponse> {
    const config = resolveRpcConfig(env, input.cluster);
    const loginResponse = await fetch(
        `${config.ephemeralRpcUrl}/auth/login`,
        {
            method: "POST",
            body: JSON.stringify(input),
        }
    );
    return loginResponse.json();
}