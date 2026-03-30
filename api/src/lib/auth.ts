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

type AuthLoginResponse = {
    token?: string;
    error?: string;
};

function buildAuthUrl(ephemeralRpcUrl: string, path: string) {
    const url = new URL(ephemeralRpcUrl);
    url.pathname = `${url.pathname.replace(/\/$/, "")}/${path}`;
    return url;
}

export function parseAuthToken(headers: Record<string, string>): string | undefined {
    const authToken = headers["Authorization"] ?? headers["authorization"];
    if (!authToken) {
        return undefined;
    }
    return authToken.split(" ")[1];
}

export async function getChallenge(env: AppEnv, input: ChallengeInput): Promise<ChallengeResponse> {
    const config = resolveRpcConfig(env, input.cluster);
    const url = buildAuthUrl(config.ephemeralRpcUrl, "auth/challenge");
    url.searchParams.set("pubkey", input.pubkey);
    const challengeResponse = await fetch(url);

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
    const { pubkey, challenge, signature } = input;
    const url = buildAuthUrl(config.ephemeralRpcUrl, "auth/login");
    const loginResponse = await fetch(url, {
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