import { AppEnv } from "../env";
import { ApiError } from "./errors";
import { resolveRpcConfig } from "./solana";

export const MOCK_AUTH_TOKEN = "mock-auth-token";
export const MOCK_CHALLENGE = "mock-challenge";
const AUTH_FETCH_TIMEOUT_MS = 5000;

export type ChallengeInput = {
    pubkey: string;
    cluster?: string;
    mock?: boolean;
};

export type ChallengeResponse = {
    challenge: string;
};

export type LoginInput = {
    pubkey: string;
    challenge: string;
    signature: string;
    cluster?: string;
    mock?: boolean;
};

export type LoginResponse = {
    token: string;
};

export type AuthChallengeResponse = {
    challenge: string;
    error?: string;
};

type AuthLoginResponse = {
    token?: string;
    error?: string;
};

function buildAuthUrl(ephemeralRpcUrl: string, path: string) {
    const url = new URL(ephemeralRpcUrl);
    url.pathname = `${url.pathname.replace(/\/$/, "")}/${path}`;
    return url;
}

async function fetchAuth(url: URL, init?: RequestInit) {
    const controller = new AbortController();
    const timeoutId = setTimeout(() => controller.abort(), AUTH_FETCH_TIMEOUT_MS);
    try {
        return await fetch(url, { ...init, signal: controller.signal });
    }
    catch (error) {
        if (error instanceof Error && error.name === "AbortError") {
            throw new ApiError(504, "RPC_ERROR", "Auth endpoint timed out");
        }
        throw error;
    }
    finally {
        clearTimeout(timeoutId);
    }
}

export function parseAuthToken(headers: Record<string, string>): string | undefined {
    const authToken = headers["Authorization"] ?? headers["authorization"];
    if (!authToken) {
        return undefined;
    }
    const parts = authToken.split(" ");
    if (parts.length !== 2 || parts[0] !== "Bearer") {
        return undefined;
    }
    return parts[1];
}

export async function getChallenge(env: AppEnv, input: ChallengeInput): Promise<ChallengeResponse> {
    if (input.mock) {
        return {
            challenge: MOCK_CHALLENGE,
        };
    }

    const config = resolveRpcConfig(env, input.cluster);
    const url = buildAuthUrl(config.ephemeralRpcUrl, "auth/challenge");
    url.searchParams.set("pubkey", input.pubkey);
    const challengeResponse = await fetchAuth(url);

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
    if (input.mock) {
        return {
            token: MOCK_AUTH_TOKEN,
        };
    }

    const config = resolveRpcConfig(env, input.cluster);
    const { pubkey, challenge, signature } = input;
    const url = buildAuthUrl(config.ephemeralRpcUrl, "auth/login");
    const loginResponse = await fetchAuth(url, {
        method: "POST",
        headers: {
            "content-type": "application/json",
        },
        body: JSON.stringify({ pubkey, challenge, signature }),
    });

    if (!loginResponse.ok) {
        throw new ApiError(loginResponse.status, "RPC_ERROR", `Failed to login: ${loginResponse.statusText}`);
    }

    const { token, error } = await loginResponse.json() as AuthLoginResponse;
    if (typeof error === "string" && error.length > 0) {
        throw new ApiError(loginResponse.status === 403 ? 403 : 502, "RPC_ERROR", `Failed to login: ${error}`);
    }
    if (typeof token !== "string" || token.length === 0) {
        throw new ApiError(502, "RPC_ERROR", "No token received");
    }

    return { token };
}