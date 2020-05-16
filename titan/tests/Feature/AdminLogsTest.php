<?php
namespace Titan\Tests\Feature;

use Illuminate\Foundation\Testing\RefreshDatabase;
use Titan\Tests\TestCase;

class AdminLogsTest extends TestCase {
    use RefreshDatabase;


    public function testPageRedirectsToLoginWhenNotLoggedIn()
    {
        $this->followRedirects = false;

        $this->get(route('admin.logs.index'))
            ->assertRedirect('/login')
            ->assertLocation(route('login'));
    }
}
