import { describe, it, expect, vi, beforeEach } from "vitest";
import { setToken, getToken, apiFetch, fsList, fsRead, fsWrite } from "./api";

beforeEach(() => {
  localStorage.clear();
  vi.restoreAllMocks();
});

describe("token storage", () => {
  it("stores and reads token", () => {
    setToken("abc");
    expect(getToken()).toBe("abc");
  });
});

describe("apiFetch", () => {
  it("attaches bearer header", async () => {
    setToken("secret");
    const fetchMock = vi.fn().mockResolvedValue(
      new Response(JSON.stringify({ ok: true }), { status: 200 })
    );
    vi.stubGlobal("fetch", fetchMock);
    await apiFetch("/api/plugins");
    const headers = fetchMock.mock.calls[0][1].headers;
    expect(headers.Authorization).toBe("Bearer secret");
  });

  it("clears token and throws on 401", async () => {
    setToken("secret");
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(new Response("", { status: 401 })));
    await expect(apiFetch("/api/plugins")).rejects.toThrow("unauthorized");
    expect(getToken()).toBeNull();
  });
});

describe("fs api", () => {
  it("fsList encodes path and parses entries", async () => {
    setToken("secret");
    const entries = [{ name: "config.toml", is_dir: false, size: 12 }];
    const fetchMock = vi.fn().mockResolvedValue(
      new Response(JSON.stringify(entries), { status: 200 })
    );
    vi.stubGlobal("fetch", fetchMock);
    const got = await fsList("kovi-plugin-ai/子目录");
    expect(got).toEqual(entries);
    expect(fetchMock.mock.calls[0][0]).toBe(
      `/api/fs/list?path=${encodeURIComponent("kovi-plugin-ai/子目录")}`
    );
  });

  it("fsRead surfaces server error message", async () => {
    setToken("secret");
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(
      new Response(JSON.stringify({ error: "非文本文件" }), { status: 400 })
    ));
    await expect(fsRead("a/blob.bin")).rejects.toThrow("非文本文件");
  });

  it("fsWrite PUTs raw text body", async () => {
    setToken("secret");
    const fetchMock = vi.fn().mockResolvedValue(new Response("", { status: 200 }));
    vi.stubGlobal("fetch", fetchMock);
    await fsWrite("a/config.toml", "x = 1\n");
    const [url, init] = fetchMock.mock.calls[0];
    expect(url).toBe(`/api/fs/write?path=${encodeURIComponent("a/config.toml")}`);
    expect(init.method).toBe("PUT");
    expect(init.body).toBe("x = 1\n");
    expect(init.headers["Content-Type"]).toBe("text/plain");
  });
});
